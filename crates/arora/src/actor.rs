use std::{future::Future, ops::Deref};

use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub struct Addr<Msg: Send>(mpsc::Sender<Msg>);

impl<Msg: Send> Addr<Msg> {
  pub fn new(tx: mpsc::Sender<Msg>) -> Self {
    Self(tx)
  }

  pub async fn send(&self, msg: Msg) -> Result<(), mpsc::error::SendError<Msg>> {
    self.0.send(msg).await
  }
}

impl<Msg: Send> Clone for Addr<Msg> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

pub trait Actor {
  type Msg: Send;

  fn spawn(self) -> Addr<Self::Msg>;
}

pub enum ReplyError {
  SendError,
}


#[derive(Debug)]
pub struct Request<I, O: Send> {
  pub data: I,
  pub reply: oneshot::Sender<O>,
}

impl<I, O: Send> Request<I, O> {
  pub fn new(data: I, reply: oneshot::Sender<O>) -> Self {
    Self { data, reply }
  }

  pub fn split(self) -> (I, oneshot::Sender<O>) {
    (self.data, self.reply)
  }

  pub async fn pipe<F>(self, future: F) -> Result<(), ReplyError>
  where
    F: Future<Output = O>,
  {
    self
      .reply
      .send(future.await)
      .map_err(|_| ReplyError::SendError)
  }
}

impl<I, O: Send> Deref for Request<I, O> {
  type Target = I;

  fn deref(&self) -> &Self::Target {
    &self.data
  }
}
