//! Client side api
//!
//! The main entry point is [RpcClient].
use crate::{
    transport::{boxed::BoxableConnection, mapped::MappedConnection, ConnectionCommon},
    Service, ServiceConnection,
};
use futures_lite::Stream;
use futures_sink::Sink;

use pin_project::pin_project;
use std::{
    fmt::Debug,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

/// Type alias for a boxed connection to a specific service
///
/// This is a convenience type alias for a boxed connection to a specific service.
pub type BoxedServiceConnection<S> =
    crate::transport::boxed::Connection<<S as crate::Service>::Res, <S as crate::Service>::Req>;

/// Sync version of `future::stream::BoxStream`.
pub type BoxStreamSync<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + Sync + 'a>>;

/// A client for a specific service
///
/// This is a wrapper around a [ServiceConnection] that serves as the entry point
/// for the client DSL.
///
/// Type parameters:
///
/// `S` is the service type that determines what interactions this client supports.
/// `C` is the substream source.
#[derive(Debug)]
pub struct RpcClient<S, C = BoxedServiceConnection<S>> {
    pub(crate) source: C,
    pub(crate) _p: PhantomData<S>,
}

impl<S, C: Clone> Clone for RpcClient<S, C> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            _p: PhantomData,
        }
    }
}

/// Sink that can be used to send updates to the server for the two interaction patterns
/// that support it, [crate::message::ClientStreaming] and [crate::message::BidiStreaming].
#[pin_project]
#[derive(Debug)]
pub struct UpdateSink<C, T>(#[pin] pub C::SendSink, PhantomData<T>)
where
    C: ConnectionCommon;

impl<C, T> UpdateSink<C, T>
where
    C: ConnectionCommon,
    T: Into<C::Out>,
{
    /// Create a new update sink
    pub fn new(sink: C::SendSink) -> Self {
        Self(sink, PhantomData)
    }
}

impl<C, T> Sink<T> for UpdateSink<C, T>
where
    C: ConnectionCommon,
    T: Into<C::Out>,
{
    type Error = C::SendError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        let req = item.into();
        self.project().0.start_send(req)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_close(cx)
    }
}

impl<S, C> RpcClient<S, C>
where
    S: Service,
    C: ServiceConnection<S>,
{
    /// Create a new rpc client for a specific [Service] given a compatible
    /// [ServiceConnection].
    ///
    /// This is where a generic typed connection is converted into a client for a specific service.
    ///
    /// You can get a client for a nested service by calling [map](RpcClient::map).
    pub fn new(source: C) -> Self {
        Self {
            source,
            _p: PhantomData,
        }
    }
}

use crate::transport::mapped::ConnectionMapExt;

impl<S, C> RpcClient<S, C>
where
    S: Service,
    C: ServiceConnection<S>,
{
    /// Get the underlying connection
    pub fn into_inner(self) -> C {
        self.source
    }

    /// Map this channel's service into an inner service.
    ///
    /// This method is available if the required bounds are upheld:
    /// SNext::Req: Into<S::Req> + TryFrom<S::Req>,
    /// SNext::Res: Into<S::Res> + TryFrom<S::Res>,
    ///
    /// Where SNext is the new service to map to and S is the current inner service.
    ///
    /// This method can be chained infintely.
    pub fn map<SNext>(self) -> RpcClient<SNext, MappedConnection<SNext::Res, SNext::Req, C>>
    where
        SNext: Service,
        S::Req: From<SNext::Req>,
        SNext::Res: TryFrom<S::Res>,
    {
        RpcClient::new(self.source.map::<SNext::Res, SNext::Req>())
    }

    /// box
    pub fn boxed(self) -> RpcClient<S, BoxedServiceConnection<S>>
    where
        C: BoxableConnection<S::Res, S::Req>,
    {
        RpcClient::new(self.source.boxed())
    }
}

impl<S, C> AsRef<C> for RpcClient<S, C>
where
    S: Service,
    C: ServiceConnection<S>,
{
    fn as_ref(&self) -> &C {
        &self.source
    }
}

/// Wrap a stream with an additional item that is kept alive until the stream is dropped
#[pin_project]
pub(crate) struct DeferDrop<S: Stream, X>(#[pin] pub S, pub X);

impl<S: Stream, X> Stream for DeferDrop<S, X> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().0.poll_next(cx)
    }
}
