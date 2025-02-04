pub mod chat;
pub mod generate;
pub mod models;

use std::cell::Cell;

use futures::{stream, Stream, StreamExt};
use node_bridge::{
    http_client::{HttpMethod, HttpResponse},
    prelude::console,
};
use wasm_bindgen::JsValue;

use crate::request::make_request;

use self::models::RequestBody;

struct ResponseState {
    response: HttpResponse,
    started: Cell<bool>,
    ended: Cell<bool>,
    first_newline_dropped: Cell<bool>,
    expect_begin_message: bool,
}

impl ResponseState {
    fn new(response: HttpResponse, expect_begin_message: bool) -> Self {
        Self {
            response,
            started: Cell::new(false),
            ended: Cell::new(false),
            first_newline_dropped: Cell::new(false),
            expect_begin_message,
        }
    }

    pub fn data_stream(&mut self) -> impl Stream<Item = String> + '_ {
        #[cfg(debug_assertions)]
        if !self.expect_begin_message {
            console::log_str(&format!("ignore begin message"));
        }
        self.started.set(!self.expect_begin_message);

        self.response.body().flat_map(|chunk| {
            let chunk = chunk.to_string("utf-8");
            #[cfg(debug_assertions)]
            console::log_str(&chunk);

            let lines: Vec<_> = chunk
                .split("\n")
                .filter_map(|l| {
                    if l.len() > 0 && l.starts_with("data: \"") {
                        serde_json::from_str::<String>(&l["data: ".len()..]).ok()
                    } else {
                        None
                    }
                })
                .filter(|s| {
                    if self.ended.get() {
                        return false;
                    }
                    if s == "<|BEGIN_message|>" {
                        self.started.set(true);
                        return false;
                    }
                    if s == "<|END_message|>" {
                        self.ended.set(true);
                        return false;
                    }
                    if !self.started.get() {
                        return false;
                    }
                    // Server may produce newlines at the head of response, we need
                    // to do this trick to ignore them in the final edit.
                    if !self.first_newline_dropped.get()
                        && s.trim().is_empty()
                        && self.expect_begin_message
                    {
                        self.first_newline_dropped.set(true);
                        return false;
                    }
                    true
                })
                .collect();
            stream::iter(lines)
        })
    }

    pub async fn complete(self) -> Result<(), JsValue> {
        self.response.await
    }
}

async fn make_conversation_request(
    path: &str,
    body: &RequestBody,
    expect_begin_message: bool,
) -> Result<ResponseState, JsValue> {
    let response = make_request(path, body, HttpMethod::Post).await?;
    if response.status_code() != 200 {
        return Err(js_sys::Error::new(&format!(
            "Server returned status code {}",
            response.status_code()
        ))
        .into());
    }
    Ok(ResponseState::new(response, expect_begin_message))
}
