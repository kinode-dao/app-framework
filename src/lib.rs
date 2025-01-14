pub use kinode_process_lib::*;
pub use process_macros::*;

/// Macro for creating a single request enum to handle all request types
/// that this app will receive.
#[macro_export]
macro_rules! req {
    ($(($name:ident, $variant:path)),*) => {
        #[derive(Debug, Deserialize, Serialize)]
        #[serde(untagged)]
        enum Req {
            $($name($variant)),*
        }
    }
}

/// Macro for creating a standard Kinode application structure
///
/// Takes an app name, icon, widget, and 2 or 3 handler functions to create a basic
/// application component that implements the Guest trait.
///
/// Three variants are supported:
/// - 5 arguments: name, icon, widget, API handler, remote request handler
/// - 6 arguments: name, icon, widget, API handler, local request handler, remote request handler
/// - 7 arguments: name, icon, widget, API handler, local request handler, remote request handler, error handler
///
/// `handle_api_call`: `impl Fn(&Message, &mut S, T1) -> (http::server::HttpResponse, Vec<u8>)`,
/// `handle_remote_request`: `impl Fn(&Message, &mut S, &mut http::server::HttpServer, T2)`,
/// `handle_send_error`: `impl Fn(&mut S, &mut http::server::HttpServer, SendError)`,
#[macro_export]
macro_rules! app {
    ($app_name:expr, $app_icon:expr, $app_widget:expr, $f1:ident, $f2:ident) => {
        struct Component;
        impl Guest for Component {
            fn init(our: String) {
                let our: Address = our.parse().unwrap();
                let init = app(
                    $app_name,
                    $app_icon,
                    $app_widget,
                    $f1,
                    |_, _, _, _: ()| {},
                    $f2,
                    |_, _, _| {},
                );
                init(our);
            }
        }
        export!(Component);
    };
    ($app_name:expr, $app_icon:expr, $app_widget:expr, $f1:ident, $f2:ident, $f3:ident) => {
        struct Component;
        impl Guest for Component {
            fn init(our: String) {
                let our: Address = our.parse().unwrap();
                let init = app(
                    $app_name,
                    $app_icon,
                    $app_widget,
                    $f1,
                    $f2,
                    $f3,
                    |_, _, _| {},
                );
                init(our);
            }
        }
        export!(Component);
    };
    ($app_name:expr, $app_icon:expr, $app_widget:expr, $f1:ident, $f2:ident, $f3:ident, $f4:ident) => {
        struct Component;
        impl Guest for Component {
            fn init(our: String) {
                let our: Address = our.parse().unwrap();
                let init = app($app_name, $app_icon, $app_widget, $f1, $f2, $f3, $f4);
                init(our);
            }
        }
        export!(Component);
    };
}

/// Trait that must be implemented by application state types
pub trait State {
    /// Creates a new instance of the state.
    fn new() -> Self;
}

/// Creates a standard Kinode application with HTTP server and WebSocket support
///
/// # Type Parameters
/// - `S`: Application state type that implements State + Serialize + Deserialize
/// - `T1`: API call payload type that implements Serialize + Deserialize
/// - `T2`: Remote request payload type that implements Serialize + Deserialize
///
/// # Arguments
/// * `app_name` - Name of the application
/// * `app_icon` - Optional icon for the application
/// * `app_widget` - Optional widget specification
/// * `handle_api_call` - Function to handle incoming HTTP API calls
/// * `handle_remote_request` - Function to handle incoming remote requests
/// * `handle_send_error` - Function to handle message send errors
pub fn app<S, T1, T2, T3>(
    app_name: &str,
    app_icon: Option<&str>,
    app_widget: Option<&str>,
    handle_api_call: impl Fn(&mut S, T1) -> (http::server::HttpResponse, Vec<u8>),
    handle_local_request: impl Fn(&Message, &mut S, &mut http::server::HttpServer, T2),
    handle_remote_request: impl Fn(&Message, &mut S, &mut http::server::HttpServer, T3),
    handle_send_error: impl Fn(&mut S, &mut http::server::HttpServer, SendError),
) -> impl Fn(Address)
where
    S: State + std::fmt::Debug + serde::Serialize + serde::de::DeserializeOwned,
    T1: serde::Serialize + serde::de::DeserializeOwned,
    T2: serde::Serialize + serde::de::DeserializeOwned,
    T3: serde::Serialize + serde::de::DeserializeOwned,
{
    homepage::add_to_homepage(app_name, app_icon, Some("/"), app_widget);
    move |our: Address| {
        let mut server = http::server::HttpServer::new(5);

        server
            .serve_ui(
                &our,
                "ui",
                vec!["/"],
                http::server::HttpBindingConfig::default(),
            )
            .expect("failed to serve UI. do you have messaging capabilities to/from http-server:distro:sys?");

        server
            .bind_http_path("/api", http::server::HttpBindingConfig::default())
            .expect("failed to serve API path");

        server
            .bind_ws_path("/updates", http::server::WsBindingConfig::default())
            .expect("failed to bind WS path");

        let mut state = get_typed_state(|bytes| serde_json::from_slice(bytes)).unwrap_or({
            let state = S::new();
            set_state(&serde_json::to_vec(&state).expect("failed to serialize state to bytes"));
            state
        });

        loop {
            match await_message() {
                Err(send_error) => handle_send_error(&mut state, &mut server, send_error),
                Ok(ref message) => handle_message(
                    &our,
                    &mut state,
                    message,
                    &mut server,
                    &handle_api_call,
                    &handle_local_request,
                    &handle_remote_request,
                ),
            }
        }
    }
}

/// Handles incoming messages by routing them to appropriate handlers
///
/// Routes local messages from the HTTP server to the API handler and
/// remote messages to the remote request handler.
///
/// # Arguments
/// * `our` - This process's address
/// * `state` - Mutable reference to application state
/// * `message` - The incoming message to handle
/// * `server` - Mutable reference to the HTTP server
/// * `handle_api_call` - Function to handle API calls
/// * `handle_remote_request` - Function to handle remote requests
fn handle_message<S, T1, T2, T3>(
    our: &Address,
    state: &mut S,
    message: &Message,
    server: &mut http::server::HttpServer,
    handle_api_call: impl Fn(&mut S, T1) -> (http::server::HttpResponse, Vec<u8>),
    handle_local_request: impl Fn(&Message, &mut S, &mut http::server::HttpServer, T2),
    handle_remote_request: impl Fn(&Message, &mut S, &mut http::server::HttpServer, T3),
) where
    S: std::fmt::Debug,
    T1: serde::Serialize + serde::de::DeserializeOwned,
    T2: serde::Serialize + serde::de::DeserializeOwned,
    T3: serde::Serialize + serde::de::DeserializeOwned,
{
    if message.is_local(our) {
        // handle local messages
        if message.source().process == "http-server:distro:sys" {
            http_request(message, state, server, handle_api_call);
        } else {
            local_request(message, state, server, handle_local_request);
        }
    } else {
        // handle remote messages
        remote_request(message, state, server, handle_remote_request);
    }
}

/// Handles incoming HTTP requests by parsing and routing to the API handler
///
/// Deserializes the request body and passes it to the handler function,
/// then returns the response with appropriate status codes.
///
/// # Arguments
/// * `message` - The incoming HTTP request message
/// * `state` - Mutable reference to application state
/// * `server` - Mutable reference to the HTTP server
/// * `handle_api_call` - Function to handle the API call
fn http_request<S, T1>(
    message: &Message,
    state: &mut S,
    server: &mut http::server::HttpServer,
    handle_api_call: impl Fn(&mut S, T1) -> (http::server::HttpResponse, Vec<u8>),
) where
    T1: serde::Serialize + serde::de::DeserializeOwned,
{
    let http_request = serde_json::from_slice::<http::server::HttpServerRequest>(&message.body())
        .expect("failed to parse HTTP request");

    server.handle_request(
        http_request,
        |_incoming| {
            let response = http::server::HttpResponse::new(200 as u16);

            let Some(blob) = message.blob() else {
                return (response.set_status(400), None);
            };

            let Ok(call) = serde_json::from_slice::<T1>(blob.bytes()) else {
                return (response.set_status(400), None);
            };

            let (response, bytes) = handle_api_call(state, call);
            (
                response,
                Some(LazyLoadBlob::new(Some("application/json"), bytes)),
            )
        },
        |_, _, _| {
            // skip incoming ws requests
        },
    );
}

/// Handles incoming local requests by deserializing and passing to handler
///
/// # Arguments
/// * `message` - The incoming local request message
/// * `state` - Mutable reference to application state
/// * `server` - Mutable reference to the HTTP server
/// * `handle_local_request` - Function to handle the local request
fn local_request<S, T>(
    message: &Message,
    state: &mut S,
    server: &mut http::server::HttpServer,
    handle_local_request: impl Fn(&Message, &mut S, &mut http::server::HttpServer, T),
) where
    S: std::fmt::Debug,
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let Ok(request) = serde_json::from_slice::<T>(&message.body()) else {
        // debug command that prints state --
        // more app-level commands can be added here
        if message.body() == b"debug" {
            kiprintln!("state:\n{:#?}", state);
        }
        return;
    };
    handle_local_request(message, state, server, request);
}

/// Handles incoming remote requests by deserializing and passing to handler
///
/// # Arguments
/// * `message` - The incoming remote request message
/// * `state` - Mutable reference to application state
/// * `server` - Mutable reference to the HTTP server
/// * `handle_remote_request` - Function to handle the remote request
fn remote_request<S, T>(
    message: &Message,
    state: &mut S,
    server: &mut http::server::HttpServer,
    handle_remote_request: impl Fn(&Message, &mut S, &mut http::server::HttpServer, T),
) where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let Ok(request) = serde_json::from_slice::<T>(&message.body()) else {
        return;
    };
    handle_remote_request(message, state, server, request);
}

/// Sends a WebSocket update to all connected clients
///
/// # Arguments
/// * `server` - Reference to the HTTP server
/// * `bytes` - The message payload to send
pub fn send_ws_update<B>(server: &http::server::HttpServer, bytes: B)
where
    B: Into<Vec<u8>>,
{
    server.ws_push_all_channels(
        "/updates",
        http::server::WsMessageType::Text,
        LazyLoadBlob::new(Some("application/json"), bytes),
    );
}
