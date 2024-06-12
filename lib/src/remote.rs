use std::collections::HashMap;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use bincode::{config, Decode, Encode};
use bincode::error::{DecodeError, EncodeError};
use futures::lock::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use tokio::sync::oneshot::Sender;
use crate::{ActorRef, Handler};

#[derive(Debug)]
pub struct ActorServer<Actor, Message, State, Response, Error> {
    actor_ref: Mutex<Option<Arc<crate::ActorRef<Actor, Message, State, Response, Error>>>>,
    failed: Mutex<bool>,
}


impl<Actor: Handler<Actor, Message, State, Response, Error> + Debug + Send + Sync + 'static, Message: Debug + Encode + Decode  + Send + Sync + 'static, State: Debug + Encode + Decode  + Send + Sync + 'static,
    Response: Debug + Encode + Decode  + Send + Sync + 'static, Error: std::error::Error + Debug + Encode + Decode  + Send + Sync + From<std::io::Error> + 'static>
ActorServer<Actor, Message, State, Response, Error> {
    pub async fn new(name: impl AsRef<str>, host: impl AsRef<str>, port: u16, actor: Arc<ActorRef<Actor,Message, State, Response, Error>>) -> Result<Arc<Self>, Error>
    {
        let name = name.as_ref().to_string();
        let address = format!("{}:{}", host.as_ref(), port);
        let listener = TcpListener::bind(address).await?;
        let handle = tokio::runtime::Handle::current();
        let actor_server = Arc::new(Self {
            actor_ref: Mutex::new(Some(actor)),
            failed: Mutex::new(false),
        });
        let actor_server_clone = actor_server.clone();
        let _handle_loop = handle.spawn(async move {
            let handle = tokio::runtime::Handle::current();
            loop {
                let accept_result = listener.accept().await;
                match accept_result {
                    Ok((socket_mut, address)) => {
                        let (mut read_half,write_half) = tokio::io::split(socket_mut);
                        let write_half_arc = Arc::new(Mutex::new(write_half));
                        let name = name.clone();
                        let actor_server_clone2 = actor_server_clone.clone();
                        handle.spawn(async move {
                            log::info!("<{name}> Connection opened from {address:?}");
                            let packet_size = std::mem::size_of::<usize>();
                            let mut buffer:Vec<u8> = vec![0; packet_size];
                            loop {
                                match read_half.read_exact(&mut buffer).await {
                                    Ok(n) => {
                                        let size = usize::from_le_bytes(buffer[..].try_into().unwrap());
                                        let mut buf:Vec<u8> = vec![0; size];
                                        if n == 0 {
                                            break;
                                        }
                                        match read_half.read_exact(&mut buf).await {
                                            Ok(n) => {
                                                if n == 0 {
                                                    break;
                                                }
                                                let message_result: Result<RequestMessage, DecodeError> = request_deserialize_command(&buf[0..n]);
                                                match message_result {
                                                    Ok(request_message) => {
                                                        match request_message.command {
                                                            RequestCommand::Send => {
                                                                let message_result:  Result<Message, DecodeError> = request_deserialize(&request_message.payload[..]);
                                                                match message_result {
                                                                    Ok(message) => {
                                                                        log::trace!("<{name}> Received (send) message: {message:?}");
                                                                        let actor_ref = actor_server_clone2.actor_ref.lock().await;
                                                                        let send_result = actor_ref.as_ref().unwrap().send(message).await;
                                                                        if send_result.is_err() {
                                                                            log::error!("<{name}> Error sending message: {:?}", send_result.err());
                                                                            break
                                                                        }
                                                                    }
                                                                    Err(err) => {
                                                                        log::error!("<{name}> Error deserializing message (payload): {:?}", err);
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                            RequestCommand::Ask => {
                                                                let message_result:  Result<Message, DecodeError> = request_deserialize(&request_message.payload[..]);
                                                                match message_result {
                                                                    Ok(message) => {
                                                                        log::debug!("<{name}> Received (ask) message: {message:?}");
                                                                        let actor_ref = actor_server_clone2.actor_ref.lock().await.clone().unwrap();
                                                                        let handel = tokio::runtime::Handle::current();
                                                                        let write_half_clone = write_half_arc.clone();
                                                                        handel.spawn(async move {
                                                                            let response = actor_ref.ask(message).await;
                                                                            let response_payload = match response {
                                                                                Ok(response) => ResponsePayload::Ok(response),
                                                                                Err(err) => ResponsePayload::Err(err),
                                                                            };
                                                                            // todo replace unwrap
                                                                            let response_data = response_serialize(request_message.id, ResponseCommand::Ask, response_payload).unwrap();


                                                                            let mut write_half = write_half_clone.lock().await;
                                                                            // todo replace unwrap
                                                                            let response_data_len_vec = response_data.len().to_le_bytes().to_vec();
                                                                            write_half.write_all(&response_data_len_vec[..]).await.unwrap();
                                                                            write_half.write_all(&response_data[..]).await.unwrap();
                                                                        });
                                                                    }
                                                                    Err(err) => {
                                                                        log::error!("<{name}> Error deserializing message (payload): {:?}", err);
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                            RequestCommand::State => {}
                                                            RequestCommand::Stop => {}
                                                        }
                                                    }
                                                    Err(err) => {
                                                        log::error!("<{name}> Error deserializing message (command): {:?}", err);
                                                        break
                                                    }
                                                }
                                                // log::info!("read end");
                                            }
                                            Err(err) => {
                                                log::trace!("<{name}> Error reading from socket: {:?}", err);
                                                break
                                            }
                                        }
                                    }

                                    Err(err) => {
                                        log::trace!("<{name}> Error reading from socket: {:?}", err);
                                        break
                                    }
                                }

                            }
                            log::info!("<{name}> Connection closed from {address:?}");
                        });
                    }
                    Err(err) => {
                        log::error!("<{name}> Error accepting connection: {:?}", err);
                        let mut failed = actor_server_clone.failed.lock().await;
                        *failed = true;
                        break
                    }
                }
            }
        });



        Ok(actor_server.clone())
    }
    pub async fn stop(&self) -> Result<(), Error> {
        if let Some(actor_ref) = self.actor_ref.lock().await.as_ref() {
            actor_ref.stop().await?;
        }
        *self.actor_ref.lock().await = None;
        Ok(())
    }
}


#[derive(Debug)]
pub struct ActorClient<Actor, Message, State, Response, Error> {
    _marker_actor: PhantomData<Actor>,
    _marker_message: PhantomData<Message>,
    _marker_state: PhantomData<State>,
    _marker_response: PhantomData<Response>,
    _marker_error: PhantomData<Error>,
    read_half: Mutex<tokio::io::ReadHalf<TcpStream>>,
    write_half: Mutex<tokio::io::WriteHalf<TcpStream>>,
    counter: Mutex<u64>,
    name: String,
    promise: Mutex<HashMap<u64, Sender<Result<Response, Error>>>>,
}


impl<Actor: Handler<Actor, Message, State, Response, Error> + Debug + Send + Sync + 'static, Message: Debug + Encode + Decode + Send + Sync + 'static, State: Debug + Encode + Decode + Send + Sync + 'static,
    Response: Debug + Encode + Decode + Send + Sync + 'static, Error: std::error::Error + Debug + Encode + Decode + Send + Sync + From<std::io::Error> + 'static>
ActorClient<Actor, Message, State, Response, Error> {
    pub async fn new(name: impl AsRef<str>, host: impl AsRef<str>, port: u16) -> Result<Arc<Self>, Error>
    {
        let name = name.as_ref().to_string();
        let address = format!("{}:{}", host.as_ref(), port);
        let stream = TcpStream::connect(address.clone()).await?;
        log::info!("<{name}> Bind at {address}");

        let (read_half, write_half) = tokio::io::split(stream);

        let actor = Arc::new(Self {
            _marker_actor: PhantomData,
            _marker_message: PhantomData,
            _marker_state: PhantomData,
            _marker_response: PhantomData,
            _marker_error: PhantomData,
            read_half: Mutex::new(read_half),
            write_half: Mutex::new(write_half),
            name: name.clone(),
            counter: Mutex::new(0),
            promise: Mutex::new(HashMap::new()),
        });

        let handle = tokio::runtime::Handle::current();
        let actor_clone = actor.clone();
        let _handle_loop = handle.spawn(async move {
            loop {
                let mut stream = actor_clone.read_half.lock().await;
                let packet_size = std::mem::size_of::<usize>();
                let mut buffer: Vec<u8> = vec![0; packet_size];
                match stream.read_exact(&mut buffer).await {
                    Ok(n) => {
                        if n == 0 {
                            break;
                        }
                        let size = usize::from_le_bytes(buffer[..].try_into().unwrap());
                        let mut buf: Vec<u8> = vec![0; size];

                        match stream.read(&mut buf).await {
                            Ok(n) => {
                                if n == 0 {
                                    break;
                                }
                                let (id, response_command, response_payload) = response_deserialize::<Response, Error>(&buf[0..n]).unwrap();
                                log::debug!("<{name}> Received message: {response_command:?}");
                                match response_command {
                                    ResponseCommand::Ask => {
                                        let mut promise = actor_clone.promise.lock().await;
                                        let sender = promise.remove(&id).unwrap();
                                        match response_payload {
                                            ResponsePayload::Ok(response) => {
                                                sender.send(Ok(response)).unwrap();
                                            }
                                            ResponsePayload::Err(err) => {
                                                sender.send(Err(err)).unwrap();
                                            }
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                log::trace!("<{name}> Error reading from socket: {:?}", err);
                                break
                            }
                        }

                    }
                    Err(err) => {
                        log::trace!("<{name}> Error reading from socket: {:?}", err);
                        break
                    }
                }


            }
            log::info!("<{name}> Connection closed");
        });
        Ok(actor)
    }


    pub async fn ask(&self, msg: Message) -> Result<Response, Error>
    {
        let (sender, receiver) = oneshot::channel();
        {

            let counter = {
                let mut counter = self.counter.lock().await;
                *counter += 1;
                *counter
            };
            let name = &self.name;
            {
                let mut stream = self.write_half.lock().await;
                let data = request_serialize(counter, RequestCommand::Ask, Some(&msg)).map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
                let data_len_vec = data.len().to_le_bytes().to_vec();
                stream.write_all(&data_len_vec[..]).await?;
                stream.write_all(&data[..]).await?;
            }
            log::debug!("<{name}> Ask message");
            self.promise.lock().await.insert(counter, sender);
        }
        let r = receiver.await;
        match r {
            Ok(res) => { res }
            Err(_) => {
                return Err(std::io::Error::new(std::io::ErrorKind::Other, "Err").into());
            }
        }
    }

    pub async fn send(&self, msg: Message) -> Result<(), std::io::Error> {
        let counter = {
            let mut counter = self.counter.lock().await;
            *counter += 1;
            *counter
        };
        let name = &self.name;
        {
            let mut stream = self.write_half.lock().await;
            let data = request_serialize(counter, RequestCommand::Send, Some(&msg)).map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
            let data_len_vec: Vec<u8> = data.len().to_le_bytes().to_vec();
            stream.write_all(&data_len_vec[..]).await?;
            stream.write_all(&data[..]).await?;
        }
        log::debug!("<{name}> Sent message");
        Ok(())
    }

    pub async fn state(&self) -> Result<Arc<Mutex<State>>, std::io::Error> {
        todo!()
    }
    pub async fn stop(&self) -> Result<(), Error> {
        self.promise.lock().await.clear();
        self.write_half.lock().await.shutdown().await?;
        log::debug!("<{}> Stop worker", self.name);
        Ok(())
    }


}

#[derive(Encode, Decode, PartialEq, Debug)]
enum RequestCommand {
    Send,
    Ask,
    State,
    Stop,
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct RequestMessage {
    id: u64,
    command: RequestCommand,
    payload: Vec<u8>,
}

fn request_serialize<T: Encode + Decode>(id: u64, command: RequestCommand, payload: Option<&T>) -> Result<Vec<u8>, EncodeError> {
    let config = config::standard();
    let encoded: Vec<u8> = if payload.is_some() {
        bincode::encode_to_vec(payload.unwrap(), config)?
    } else {
        vec![]
    };
    let request_message = RequestMessage {
        id,
        command,
        payload: encoded,
    };
    let request_message_encoded = bincode::encode_to_vec(&request_message, config)?;
    Ok(request_message_encoded)
}

fn request_deserialize_command(data: &[u8]) -> Result<RequestMessage, DecodeError> {
    let config = config::standard();
    let (request_message, _): (RequestMessage, _) = bincode::decode_from_slice(data, config)?;
    Ok(request_message)
}

fn request_deserialize<T: Encode + Decode>(data: &[u8]) -> Result<T, DecodeError> {
    let config = config::standard();
    let (payload, _): (T, _) = bincode::decode_from_slice(data, config)?;
    Ok(payload)
}

#[derive(Encode, Decode, PartialEq, Debug)]
struct ResponseMessage {
    id: u64,
    command: ResponseCommand,
    payload: Vec<u8>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
enum ResponseCommand {
    Ask,
}

#[derive(Encode, Decode, PartialEq, Debug)]
enum ResponsePayload<R,E> {
    Ok(R),
    Err(E),
}

fn response_serialize<Response: Encode + Decode, Error: Encode + Decode>(id: u64, command: ResponseCommand, payload: ResponsePayload<Response, Error>) -> Result<Vec<u8>, EncodeError> {
    let config = config::standard();
    let encoded: Vec<u8> = bincode::encode_to_vec(payload, config)?;
    let response_message = ResponseMessage {
        id,
        command,
        payload: encoded,
    };
    let response_message_encoded = bincode::encode_to_vec(&response_message, config)?;
    Ok(response_message_encoded)
}

fn response_deserialize<Response: Encode + Decode, Error: Encode + Decode>(data: &[u8]) -> Result<(u64, ResponseCommand, ResponsePayload<Response, Error>), DecodeError> {
    let config = config::standard();
    let (response_message, _): (ResponseMessage, _) = bincode::decode_from_slice(data, config)?;
    let (payload, _): (ResponsePayload<Response, Error>, _) = bincode::decode_from_slice(&response_message.payload[..],config)?;
    Ok((response_message.id, response_message.command, payload))
}
