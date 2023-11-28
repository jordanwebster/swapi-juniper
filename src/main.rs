use juniper::graphql_object;
use juniper::EmptyMutation;
use juniper::EmptySubscription;
use juniper::RootNode;
use warp::Filter;

#[derive(Clone)]
struct Context;

impl juniper::Context for Context {}

struct Query;

#[graphql_object(context = Context)]
impl Query {
    fn hello() -> &'static str {
        "hello world"
    }
}

type Schema = RootNode<'static, Query, EmptyMutation<Context>, EmptySubscription<Context>>;

fn schema() -> Schema {
    Schema::new(Query, EmptyMutation::new(), EmptySubscription::new())
}

#[tokio::main]
async fn main() {
    let routes = (warp::post()
        .and(warp::path("graphql"))
        .and(juniper_warp::make_graphql_filter(
            schema(),
            warp::any().map(|| Context).boxed(),
        )))
    .or(warp::get()
        .and(warp::path("playground"))
        .and(juniper_warp::playground_filter("/graphql", None)));

    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}
