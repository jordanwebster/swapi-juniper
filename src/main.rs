use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use juniper::graphql_interface;
use juniper::graphql_object;
use juniper::EmptyMutation;
use juniper::EmptySubscription;
use juniper::GraphQLObject;
use juniper::RootNode;
use juniper::ID;
use regex::Regex;
use serde::Deserialize;
use warp::Filter;

fn parse_id(id: &str) -> u32 {
    let re = Regex::new(r"https://swapi.dev/api/[^/]+/(?<id>\d+)/").unwrap();
    re.captures(id).unwrap()["id"].parse().unwrap()
}

#[derive(Deserialize, Clone)]
#[serde(tag = "type")]
enum NodeJson {
    Person(Person),
    Film(Film),
    Planet,
    Species,
    Starship,
    Vehicle,
}

impl From<NodeJson> for NodeValue {
    fn from(value: NodeJson) -> Self {
        match value {
            NodeJson::Film(film) => NodeValue::Film(film),
            NodeJson::Person(person) => NodeValue::Person(person),
            _ => todo!(),
        }
    }
}

#[derive(Clone)]
struct Context {
    data: Arc<BTreeMap<String, NodeJson>>,
}

impl juniper::Context for Context {}

#[graphql_interface(for = [Film, Person], context = Context)]
trait Node {
    fn id(&self) -> &ID;
}

#[derive(Deserialize, Clone)]
struct Film {
    id: ID,
    title: String,
    characters: Vec<String>,
}

#[graphql_object(context = Context)]
#[graphql(impl = NodeValue)]
impl Film {
    fn title(&self) -> &str {
        &self.title
    }

    fn characters(
        &self,
        context: &Context,
        after: Option<String>,
        first: Option<i32>,
        before: Option<String>,
        last: Option<i32>,
    ) -> PersonConnection {
        let first_id = parse_id(self.characters.first().unwrap());
        let last_id = parse_id(self.characters.last().unwrap());

        let mut edges: Box<dyn Iterator<Item = &String>> = Box::new(self.characters.iter());
        if let Some(ref after) = after {
            edges = Box::new(edges.filter(|id| parse_id(id) > parse_id(after)));
        }
        if let Some(ref before) = before {
            edges = Box::new(edges.filter(|id| parse_id(id) < parse_id(before)));
        }
        if let Some(first) = first {
            edges = Box::new(edges.take(first as usize));
        }
        if let Some(last) = last {
            edges = Box::new(
                edges
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .take(last as usize)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev(),
            );
        }

        let characters = edges
            .map(|id| match context.data.get(id) {
                Some(NodeJson::Person(person)) => person,
                _ => panic!("{} is not a Person", id),
            })
            .collect::<Vec<_>>();

        let page_first_id = characters.first().map(|c| parse_id(&c.id));
        let page_last_id = characters.last().map(|c| parse_id(&c.id));

        PersonConnection {
            page_info: PageInfo {
                has_previous_page: if let Some(start) = page_first_id {
                    start > first_id
                } else {
                    false
                },
                has_next_page: if let Some(end) = page_last_id {
                    end < last_id
                } else {
                    false
                },
                start_cursor: characters.first().map(|c| c.id.to_string()),
                end_cursor: characters.last().map(|c| c.id.to_string()),
            },
            edges: characters
                .into_iter()
                .map(|c| PersonEdge {
                    cursor: c.id.to_string(),
                    node: Some(c.clone()),
                })
                .collect::<Vec<_>>(),
        }
    }
}

impl Node for Film {
    fn id(&self) -> &ID {
        &self.id
    }
}

#[derive(GraphQLObject, Deserialize, Clone)]
#[graphql(impl = NodeValue, context = Context)]
struct Person {
    id: ID,
    name: String,
}

impl Node for Person {
    fn id(&self) -> &ID {
        &self.id
    }
}

#[derive(GraphQLObject, Deserialize, Clone)]
struct PageInfo {
    has_previous_page: bool,
    has_next_page: bool,
    start_cursor: Option<String>,
    end_cursor: Option<String>,
}

#[derive(GraphQLObject, Deserialize, Clone)]
#[graphql(context = Context)]
struct PersonConnection {
    page_info: PageInfo,
    edges: Vec<PersonEdge>,
}

#[derive(GraphQLObject, Deserialize, Clone)]
#[graphql(context = Context)]
struct PersonEdge {
    node: Option<Person>,
    cursor: String,
}

struct Query;

impl Query {
    fn film(context: &Context, id: ID) -> Option<Film> {
        match Self::node(context, id) {
            Some(NodeValue::Film(film)) => Some(film),
            _ => None,
        }
    }

    fn person(context: &Context, id: ID) -> Option<Person> {
        match Self::node(context, id) {
            Some(NodeValue::Person(person)) => Some(person),
            _ => None,
        }
    }

    fn node(context: &Context, id: ID) -> Option<NodeValue> {
        context
            .data
            .get(&id.to_string())
            .cloned()
            .map(|node| node.into())
    }
}

#[graphql_object(context = Context)]
impl Query {
    fn film(context: &Context, id: ID) -> Option<Film> {
        Self::film(context, id)
    }

    fn person(context: &Context, id: ID) -> Option<Person> {
        Self::person(context, id)
    }

    fn node(context: &Context, id: ID) -> Option<NodeValue> {
        Self::node(context, id)
    }
}

type Schema = RootNode<'static, Query, EmptyMutation<Context>, EmptySubscription<Context>>;

fn schema() -> Schema {
    Schema::new(Query, EmptyMutation::new(), EmptySubscription::new())
}

#[tokio::main]
async fn main() {
    let file = File::open("src/data.json").expect("data.json must be present");
    let reader = BufReader::new(file);
    let data: Arc<BTreeMap<String, NodeJson>> =
        Arc::new(serde_json::from_reader(reader).expect("data.json must be valid JSON"));
    let data = warp::any().map(move || data.clone());
    let context_extractor = warp::any()
        .and(data)
        .map(|data: Arc<BTreeMap<String, NodeJson>>| Context { data })
        .boxed();

    let routes = (warp::post()
        .and(warp::path("graphql"))
        .and(juniper_warp::make_graphql_filter(
            schema(),
            context_extractor,
        )))
    .or(warp::get()
        .and(warp::path("playground"))
        .and(juniper_warp::playground_filter("/graphql", None)));

    /*
    let s = RootNode::new(
        Query,
        EmptyMutation::<()>::new(),
        EmptySubscription::<()>::new(),
    );
    println!("{}", s.as_schema_language());
    */

    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}
