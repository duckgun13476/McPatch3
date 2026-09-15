use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::Stream;
use tokio_stream::StreamExt;

use crate::web::log::LogOutputed;
use crate::web::webstate::WebState;

fn log_event(log: LogOutputed) -> Result<Event, Infallible> {
    Ok(Event::default().json_data(log).unwrap())
}

pub async fn api_stream(
    State(state): State<WebState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (snapshot, receiver) = state.console.snapshot_and_subscribe();
    let initial = tokio_stream::iter(snapshot.into_iter().map(log_event));
    let live = BroadcastStream::new(receiver).filter_map(|result| result.ok().map(log_event));

    Sse::new(initial.chain(live)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
