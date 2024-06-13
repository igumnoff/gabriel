mod echo;

use gabriel2::*;
use echo::*;
#[tokio::main]
async fn main() -> Result<(), EchoError> {
    let state = EchoState {
        counter: 0,
    };

    let echo_ref = ActorRef::new("echo", EchoActor {}, state, 100000).await?;

    println!("Sent Ping");
    echo_ref.send(EchoMessage::Ping).await?;

    println!("Sent Ping and ask response");
    let pong = echo_ref.ask(EchoMessage::Ping).await?;
    println!("Got {:?}", pong);

    _ = echo_ref.stop().await;
    Ok(())
}


#[cfg(test)]
mod tests {
    use std::sync::{Arc};
    use gabriel2::*;

    use crate::echo::{EchoActor, EchoError, EchoMessage, EchoResponse, EchoState};


    #[tokio::test]
    async fn test_remote() -> anyhow::Result<()> {
        use gabriel2::remote::*;
        // let _ = env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("trace")).try_init();

        let state = EchoState {
            counter: 0,
        };

        let echo_ref = ActorRef::new("echo".to_string(), crate::echo::EchoActor {}, state, 100000).await?;
        let echo_server = ActorServer::new("echo_server", "127.0.0.1", 9001, echo_ref).await?;
        let echo_client: Arc<ActorClient<EchoActor, EchoMessage, EchoState, EchoResponse, EchoError >> = ActorClient::new("echo_client", "127.0.0.1", 9001).await?;

        println!("Sent Ping");
        echo_client.send(EchoMessage::Ping).await?;

        println!("Sent Ping and ask response");
        let pong = echo_client.ask(EchoMessage::Ping).await?;
        println!("Got {:?}", pong);

        _ = echo_client.stop().await;
        _ = echo_server.stop().await;
        Ok(())
    }


}

