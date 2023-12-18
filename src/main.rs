use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use juniper::graphql_interface;
use juniper::graphql_object;
use juniper::marker::IsOutputType;
use juniper::meta::MetaType;
use juniper::EmptyMutation;
use juniper::EmptySubscription;
use juniper::GraphQLObject;
use juniper::GraphQLType;
use juniper::GraphQLValue;
use juniper::GraphQLValueAsync;
use juniper::Registry;
use juniper::RootNode;
use juniper::ScalarValue;
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

trait ConnectionEdge {
    fn connection_type_name() -> &'static str;

    fn edge_type_name() -> &'static str;
}

#[derive(Deserialize, Clone)]
struct Film {
    id: ID,
    title: String,
    characters: Vec<String>,
}

impl Node for Film {
    fn id(&self) -> &ID {
        &self.id
    }
}

impl ConnectionEdge for Film {
    fn connection_type_name() -> &'static str {
        "FilmConnection"
    }

    fn edge_type_name() -> &'static str {
        "FilmEdge"
    }
}

#[graphql_object(context = Context, impl = NodeValue)]
impl Film {
    fn id(&self) -> &ID {
        &self.id
    }

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
    ) -> Connection<Person> {
        Connection::new(
            &self.characters,
            |id| match context.data.get(id) {
                Some(NodeJson::Person(person)) => person.clone(),
                _ => panic!("{} is not a Person", id),
            },
            after,
            first,
            before,
            last,
        )
    }
}

#[derive(Deserialize, Clone)]
struct Person {
    id: ID,
    name: String,
    films: Vec<String>,
}

impl Node for Person {
    fn id(&self) -> &ID {
        &self.id
    }
}
impl ConnectionEdge for Person {
    fn connection_type_name() -> &'static str {
        "PersonConnection"
    }

    fn edge_type_name() -> &'static str {
        "PersonEdge"
    }
}

#[graphql_object(context = Context, impl = NodeValue)]
impl Person {
    fn id(&self) -> &ID {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn films(
        &self,
        context: &Context,
        after: Option<String>,
        first: Option<i32>,
        before: Option<String>,
        last: Option<i32>,
    ) -> Connection<Film> {
        Connection::new(
            &self.films,
            |id| match context.data.get(id) {
                Some(NodeJson::Film(film)) => film.clone(),
                _ => panic!("{} is not a Film", id),
            },
            after,
            first,
            before,
            last,
        )
    }
}

#[derive(GraphQLObject, Deserialize, Clone)]
struct PageInfo {
    has_previous_page: bool,
    has_next_page: bool,
    start_cursor: Option<String>,
    end_cursor: Option<String>,
}

struct Edge<N> {
    node: Option<N>,
    cursor: String,
}

struct Connection<N> {
    edges: Vec<Edge<N>>,
    page_info: PageInfo,
}

impl<N: Node> Connection<N> {
    fn new(
        ids: &[String],
        load: impl Fn(&str) -> N,
        after: Option<String>,
        first: Option<i32>,
        before: Option<String>,
        last: Option<i32>,
    ) -> Connection<N> {
        let before = before.as_deref().map(parse_id);
        let after = after.as_deref().map(parse_id);
        let edges = ids
            .iter()
            .filter(|id| {
                let id = parse_id(id);
                match (before, after) {
                    (Some(before), Some(after)) => id > after && id < before,
                    (Some(before), None) => id < before,
                    (None, Some(after)) => id > after,
                    (None, None) => true,
                }
            })
            .collect::<Vec<_>>();
        if edges.is_empty() {
            return Self {
                page_info: PageInfo {
                    has_previous_page: false,
                    has_next_page: false,
                    start_cursor: None,
                    end_cursor: None,
                },
                edges: vec![],
            };
        }
        let first_id = edges.first().map(|id| parse_id(id)).unwrap();
        let last_id = edges.last().map(|id| parse_id(id)).unwrap();

        let take_length = first.map(|first| first as usize).unwrap_or(edges.len());
        let skip_length = match last {
            Some(last) => take_length.saturating_sub(last as usize),
            None => 0,
        };

        let edges = edges
            .into_iter()
            .take(take_length)
            .skip(skip_length)
            .map(|id| load(id))
            .collect::<Vec<_>>();

        let page_first_id = edges.first().map(|c| parse_id(c.id()));
        let page_last_id = edges.last().map(|c| parse_id(c.id()));

        Self {
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
                start_cursor: edges.first().map(|c| c.id().to_string()),
                end_cursor: edges.last().map(|c| c.id().to_string()),
            },
            edges: edges
                .into_iter()
                .map(|c| Edge {
                    cursor: c.id().to_string(),
                    node: Some(c),
                })
                .collect::<Vec<_>>(),
        }
    }
}

impl<N, S> GraphQLType<S> for Connection<N>
where
    N: Node + ConnectionEdge + GraphQLType<S>,
    N::Context: juniper::Context,
    S: ScalarValue,
{
    fn name(_info: &<N as GraphQLValue<S>>::TypeInfo) -> Option<&'static str> {
        Some(N::connection_type_name())
    }

    fn meta<'r>(
        info: &<N as GraphQLValue<S>>::TypeInfo,
        registry: &mut Registry<'r, S>,
    ) -> MetaType<'r, S>
    where
        S: 'r,
    {
        let fields = &[
            registry.field::<&[Edge<N>]>("edges", info),
            registry.field::<&PageInfo>("pageInfo", &()),
        ];
        registry.build_object_type::<Self>(info, fields).into_meta()
    }
}

impl<N, S> IsOutputType<S> for Connection<N>
where
    N: GraphQLType<S>,
    S: ScalarValue,
{
}

impl<N, S> GraphQLValue<S> for Connection<N>
where
    N: Node + ConnectionEdge + GraphQLType<S>,
    N::Context: juniper::Context,
    S: ScalarValue,
{
    type Context = N::Context;
    type TypeInfo = <N as GraphQLValue<S>>::TypeInfo;

    fn type_name<'i>(&self, info: &'i Self::TypeInfo) -> Option<&'i str> {
        <Self as GraphQLType<S>>::name(info)
    }

    fn resolve_field(
        &self,
        info: &Self::TypeInfo,
        field_name: &str,
        _arguments: &juniper::Arguments<S>,
        executor: &juniper::Executor<Self::Context, S>,
    ) -> juniper::ExecutionResult<S> {
        match field_name {
            "edges" => executor.resolve_with_ctx(info, &self.edges),
            "pageInfo" => executor.resolve_with_ctx(&(), &self.page_info),
            _ => panic!("Field {} not found on Connection", field_name),
        }
    }
}

impl<N, S> GraphQLValueAsync<S> for Connection<N>
where
    N: Node + ConnectionEdge + GraphQLType<S> + GraphQLValueAsync<S> + Send + Sync,
    N::TypeInfo: Sync,
    N::Context: juniper::Context + Sync,
    S: ScalarValue + Send + Sync,
{
    fn resolve_field_async<'a>(
        &'a self,
        info: &'a Self::TypeInfo,
        field_name: &'a str,
        _arguments: &'a juniper::Arguments<S>,
        executor: &'a juniper::Executor<Self::Context, S>,
    ) -> juniper::BoxFuture<'a, juniper::ExecutionResult<S>> {
        let f = async move {
            match field_name {
                "edges" => executor.resolve_with_ctx_async(info, &self.edges).await,
                "pageInfo" => executor.resolve_with_ctx(&(), &self.page_info),
                _ => panic!("Field {} not found on Connection", field_name),
            }
        };
        use ::juniper::futures::future;
        future::FutureExt::boxed(f)
    }
}

impl<N, S> GraphQLType<S> for Edge<N>
where
    N: Node + ConnectionEdge + GraphQLType<S>,
    N::Context: juniper::Context,
    S: ScalarValue,
{
    fn name(_info: &<N as GraphQLValue<S>>::TypeInfo) -> Option<&'static str> {
        Some(N::edge_type_name())
    }

    fn meta<'r>(
        info: &<N as GraphQLValue<S>>::TypeInfo,
        registry: &mut Registry<'r, S>,
    ) -> MetaType<'r, S>
    where
        S: 'r,
    {
        let fields = &[
            registry.field::<&N>("node", info),
            registry.field::<&String>("cursor", &()),
        ];
        registry.build_object_type::<Self>(info, fields).into_meta()
    }
}

impl<N, S> IsOutputType<S> for Edge<N>
where
    N: GraphQLType<S>,
    S: ScalarValue,
{
}

impl<N, S> GraphQLValue<S> for Edge<N>
where
    N: Node + ConnectionEdge + GraphQLType<S>,
    N::Context: juniper::Context,
    S: ScalarValue,
{
    type Context = N::Context;
    type TypeInfo = <N as GraphQLValue<S>>::TypeInfo;

    fn type_name<'i>(&self, info: &'i Self::TypeInfo) -> Option<&'i str> {
        <Self as GraphQLType<S>>::name(info)
    }

    fn resolve_field(
        &self,
        info: &Self::TypeInfo,
        field_name: &str,
        _arguments: &juniper::Arguments<S>,
        executor: &juniper::Executor<Self::Context, S>,
    ) -> juniper::ExecutionResult<S> {
        match field_name {
            "node" => executor.resolve_with_ctx(info, &self.node),
            "cursor" => executor.resolve_with_ctx(&(), &self.cursor),
            _ => panic!("Field {} not found on Edge", field_name),
        }
    }
}

impl<N, S> GraphQLValueAsync<S> for Edge<N>
where
    N: Node + ConnectionEdge + GraphQLType<S> + GraphQLValueAsync<S> + Send + Sync,
    N::TypeInfo: Sync,
    N::Context: juniper::Context + Sync,
    S: ScalarValue + Send + Sync,
{
    fn resolve_field_async<'a>(
        &'a self,
        info: &'a Self::TypeInfo,
        field_name: &'a str,
        _arguments: &'a juniper::Arguments<S>,
        executor: &'a juniper::Executor<Self::Context, S>,
    ) -> juniper::BoxFuture<'a, juniper::ExecutionResult<S>> {
        let f = async move {
            match field_name {
                "node" => executor.resolve_with_ctx_async(info, &self.node).await,
                "cursor" => executor.resolve_with_ctx(&(), &self.cursor),
                _ => panic!("Field {} not found on Edge", field_name),
            }
        };
        use ::juniper::futures::future;
        future::FutureExt::boxed(f)
    }
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
