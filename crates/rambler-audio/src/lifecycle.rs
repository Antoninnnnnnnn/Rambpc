use rambler_engine::{AudioSource, SourceError, SourceFuture};

/// Owns a resumable close future so cancelling a next/close future never invokes
/// upstream close twice. The owned source stays alive until its close completes.
pub(crate) struct SourceOwner<S: AudioSource> {
    pub source: Option<S>,
    closing: Option<SourceFuture<'static, Result<(), SourceError>>>,
    closed: Option<Result<(), SourceError>>,
}
impl<S: AudioSource> SourceOwner<S> {
    pub fn new(source: S) -> Self {
        Self {
            source: Some(source),
            closing: None,
            closed: None,
        }
    }
    pub async fn close(&mut self) -> Result<(), SourceError> {
        if let Some(result) = &self.closed {
            return result.clone();
        }
        if self.closing.is_none() {
            if let Some(mut source) = self.source.take() {
                self.closing = Some(Box::pin(async move { source.close().await }));
            } else {
                self.closed = Some(Ok(()));
                return Ok(());
            }
        }
        let result = match self.closing.as_mut() {
            Some(future) => future.await,
            None => Ok(()),
        };
        self.closed = Some(result.clone());
        self.closing = None;
        result
    }
}
impl<S: AudioSource> Drop for SourceOwner<S> {
    fn drop(&mut self) {
        let pending = self.closing.take().or_else(|| {
            self.source.take().map(|mut source| {
                Box::pin(async move { source.close().await })
                    as SourceFuture<'static, Result<(), SourceError>>
            })
        });
        if let Some(pending) = pending
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                let _ = pending.await;
            });
        }
    }
}
