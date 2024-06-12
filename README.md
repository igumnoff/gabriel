# Gabriel2

![Gabriel2](https://github.com/igumnoff/gabriel2/raw/HEAD/logo.png)

Gabriel2: Indeed, an actor library based on Tokio, written in Rust

## Features

- [x] Async for sending messages
- [x] Async for messages processing in actor
- [x] Support messaging like send and forget 
- [x] Support messaging like send and wait response
- [x] Mutable state of actor
- [x] Self reference in actor from context
- [x] Actor lifecycle (pre_start, pre_stop)
- [x] Remote Actor

## TODO
- [ ] Load Balancer

## Usage

Cargo.toml

```toml
[dependencies]
gabriel2 = { version = "1.1.0", features = ["remote"] }
```

echo.rs

```rust
#[derive(Debug)]
pub struct Echo;

#[derive(Debug)]
pub enum Message {
    Ping,
}

#[derive(Debug)]
pub enum Response {
    Pong {counter: u32},
}

#[derive(Debug,Clone)]
pub struct State {
    pub counter: u32,
}

#[derive(Debug, Error)]
pub enum EchoError {
    #[error("unknown error")]
    Unknown,
    #[error("std::io::Error")]
    StdErr(#[from] std::io::Error),
}

#[async_trait]
impl Handler<Echo, Message, State, Response, EchoError> for Echo {
    async fn receive(&self, ctx: Arc<Context<Echo, Message, State, Response, EchoError>>) -> Result<Response, EchoError> {
        match ctx.mgs {
            Message::Ping => {
                println!("Received Ping");
                let mut state_lock = ctx.state.lock().await;
                state_lock.counter += 1;
                if state_lock.counter > 10 {
                    Err(EchoError::Unknown)
                } else {
                    Ok(Response::Pong{counter: state_lock.counter})
                }
            }
        }
    }
}
```

main.rs

```rust
#[tokio::main]
async fn main() -> Result<(), EchoError> {
    let state = State {
        counter: 0,
    };

    let echo_ref = ActorRef::new("echo".to_string(), Echo{},  state, 100000).await?;

    println!("Sent Ping");
    echo_ref.send(Message::Ping).await?;

    println!("Sent Ping and ask response");
    let pong = echo_ref.ask(Message::Ping).await?;
    println!("Got {:?}", pong);

    _ = echo_ref.stop().await;
    Ok(())
}
```

Example output:

```text 
Sent Ping
Sent Ping and ask response
Received Ping
Received Ping
Got Pong { counter: 2 }
```

Example sources: https://github.com/igumnoff/gabriel2/tree/main/test


## Remote 

Preparations for remote:

Add Encode, Decode from "bincode" to derive(..) for Message, Response, State

Change EchoError
```rust
use derive_more::{Display, Error};
use bincode::{Decode, Encode};

#[derive(Debug, Display, Error,  Encode, Decode)]
pub enum EchoError {
    #[display(fmt = "Unknown error")]
    Unknown,
}

impl From<std::io::Error> for EchoError {
    fn from(_err: std::io::Error) -> Self {
        EchoError::Unknown
    }
}
```
Remote version:

```rust
#[tokio::main]
async fn main() -> Result<(), EchoError> {
    let state = State {
        counter: 0,
    };
    let echo_ref = ActorRef::new("echo".to_string(), crate::echo::Echo {}, state, 100000).await?;

    let echo_server = ActorServer::new("echo_server", "127.0.0.1", 9001, echo_ref).await?;

    let echo_client: Arc<ActorClient<Echo, Message, State, Response, EchoError >> = ActorClient::new("echo_client", "127.0.0.1", 9001).await?;

    println!("Sent Ping");
    echo_client.send(Message::Ping).await?;

    println!("Sent Ping and ask response");

    let pong = echo_client.ask(Message::Ping).await?;
    println!("Got {:?}", pong);

    tokio::time::sleep(Duration::from_secs(1)).await;
    _ = echo_client.stop().await;
    _ = echo_server.stop().await;
    Ok(())
}
```

## Contributing
I would love to see contributions from the community. If you experience bugs, feel free to open an issue. If you would like to implement a new feature or bug fix, please follow the steps:
1. Read "[Contributor License Agreement (CLA)](https://github.com/igumnoff/gabriel2/blob/main/CLA)"
2. Contact with me via telegram @ievkz or discord @igumnovnsk
3. Confirm e-mail invitation in repository
4. Do "git clone" (You don't need to fork!)
5. Create branch with your assigned issue
6. Create pull request to main branch
