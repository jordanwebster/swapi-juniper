use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use juniper::graphql_object;
use juniper::EmptyMutation;
use juniper::EmptySubscription;
use juniper::RootNode;
use juniper::ID;
use warp::Filter;

#[derive(Clone)]
struct Context {
    data: Arc<serde_json::Map<String, serde_json::Value>>,
}

impl juniper::Context for Context {}

struct Film {
    id: ID,
    title: String,
}

#[graphql_object(context = Context)]
impl Film {
    fn id(&self) -> &ID {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }
}

struct Person {
    id: ID,
    name: String,
}

#[graphql_object(context = Context)]
impl Person {
    fn id(&self) -> &ID {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }
}

struct Query;

#[graphql_object(context = Context)]
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
