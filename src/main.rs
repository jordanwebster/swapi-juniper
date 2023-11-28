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
use warp::Filter;

#[derive(Clone)]
struct Context {
    // TODO: Serialize the JSON into proper structs
    data: Arc<serde_json::Map<String, serde_json::Value>>,
}

impl juniper::Context for Context {}

#[graphql_interface(for = [Film, Person])]
trait Node {
    fn id(&self) -> &ID;
}

#[derive(GraphQLObject)]
#[graphql(impl = NodeValue)]
struct Film {
    id: ID,
    title: String,
}

impl Node for Film {
    fn id(&self) -> &ID {
        &self.id
    }
}

#[derive(GraphQLObject)]
#[graphql(impl = NodeValue)]
struct Person {
    id: ID,
    name: String,
}

impl Node for Person {
    fn id(&self) -> &ID {
        &self.id
    }
}

struct Query;

impl Query {
    fn film(context: &Context, id: ID) -> Option<Film> {
        context.data.get(&id.to_string()).map(|value| Film {
            id,
            title: value.get("title").unwrap().as_str().unwrap().to_string(),
        })
    }

    fn person(context: &Context, id: ID) -> Option<Person> {
        context.data.get(&id.to_string()).map(|value| Person {
            id,
            name: value.get("name").unwrap().as_str().unwrap().to_string(),
        })
    }

    fn node(context: &Context, id: ID) -> Option<NodeValue> {
        if id.to_string().contains("people") {
            Self::person(context, id).map(NodeValue::Person)
        } else {
            Self::film(context, id).map(NodeValue::Film)
        }
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
    let data: Arc<serde_json::Map<String, serde_json::Value>> =
        Arc::new(serde_json::from_reader(reader).expect("data.json must be valid JSON"));
    let data = warp::any().map(move || data.clone());
    let context_extractor = warp::any()
        .and(data)
        .map(|data: Arc<serde_json::Map<String, serde_json::Value>>| Context { data })
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

    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}
