use std::sync::Arc;

use axum::{
    extract::{State, WebSocketUpgrade, ws::Message},
    http::HeaderMap,
    response::Response,
};

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    user_events::{UserEvent, UserEventKind, topic},
};

pub async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Response, AuthError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let events = pubsub::topic::<UserEvent>(&topic(owner)).subscribe_live();
    Ok(ws.on_upgrade(move |socket| handle(socket, state, events)))
}

async fn handle(
    mut socket: axum::extract::ws::WebSocket,
    state: Arc<ServerState>,
    mut events: pubsub::Subscriber<UserEvent>,
) {
    loop {
        tokio::select! {
            message = socket.recv() => {
                if matches!(message, Some(Ok(Message::Close(_))) | None) {
                    break;
                }
            }
            event = events.recv() => {
                let event = match event {
                    Ok(event) => event,
                    Err(pubsub::RecvError::Lagged(_)) => UserEvent {
                        id: state.id_gen.new_uuid_v7(),
                        kind: UserEventKind::Resync,
                    },
                    Err(pubsub::RecvError::Decode(_)) => continue,
                    Err(pubsub::RecvError::Closed) => break,
                };
                let Ok(data) = serde_json::to_string(&event) else {
                    continue;
                };
                if socket.send(Message::Text(data.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}
