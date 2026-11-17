//! Raw localhost peer and independent upload receipts for the fixed corpus.

use super::{
    Check, Failed, Invocation, PreparedCase, SESSION_DEADLINE, audio_common, check, guarded,
};
use crate::common::{Replies, replies};
use bytes::Bytes;
use futures_util::{Stream, StreamExt, TryStreamExt, stream};
use rambler_protocol::{
    AUDIO_DATA, AUDIO_INFO, audio_request, end_request,
    wire::{Field, Value, parse_fields},
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    ops::Range,
    sync::{Arc, Mutex},
};
use tonic::{Request, Response, Status, Streaming};

const MAX_MESSAGES: u64 = 256;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_OGG_BYTES: usize = 128 * 1024;

#[derive(Default)]
struct State {
    remote: Option<SocketAddr>,
    claimed: usize,
    confirmed: usize,
    rejected: bool,
    receipt: Option<UploadReceipt>,
}

pub(super) struct Peer {
    pub cases: Vec<PreparedCase>,
    pub invocations: Vec<Invocation>,
    state: Mutex<State>,
}

impl Peer {
    pub fn new(cases: Vec<PreparedCase>, invocations: Vec<Invocation>) -> Self {
        Self {
            cases,
            invocations,
            state: Mutex::default(),
        }
    }

    fn claim(&self, remote: Option<SocketAddr>) -> Check<usize> {
        let mut state = self.state.lock().map_err(|_| Failed)?;
        if state.rejected
            || state.claimed != state.confirmed
            || state.claimed >= self.invocations.len()
            || !matches!(remote, Some(SocketAddr::V4(addr)) if *addr.ip() == Ipv4Addr::LOCALHOST)
            || state
                .remote
                .is_some_and(|previous| Some(previous) != remote)
        {
            state.rejected = true;
            return Err(Failed);
        }
        let ordinal = state.claimed;
        state.remote = remote;
        state.claimed += 1;
        Ok(ordinal)
    }

    fn record(&self, ordinal: usize, receipt: UploadReceipt) -> Check<()> {
        let mut state = self.state.lock().map_err(|_| Failed)?;
        check(!state.rejected && state.claimed == ordinal + 1 && state.receipt.is_none())?;
        state.receipt = Some(receipt);
        Ok(())
    }

    pub fn confirm(&self, ordinal: usize) -> Check<UploadReceipt> {
        let mut state = self.state.lock().map_err(|_| Failed)?;
        check(!state.rejected && state.confirmed == ordinal && state.claimed == ordinal + 1)?;
        let receipt = state.receipt.take().ok_or(Failed)?;
        state.confirmed += 1;
        Ok(receipt)
    }

    pub fn verify_closed(&self) -> Check<()> {
        let state = self.state.lock().map_err(|_| Failed)?;
        check(
            !state.rejected
                && state.claimed == self.invocations.len()
                && state.confirmed == state.claimed
                && state.receipt.is_none(),
        )
    }

    fn reply_range(
        self: Arc<Self>,
        case_index: usize,
        indices: Range<usize>,
    ) -> impl Stream<Item = Result<Bytes, Status>> + Send + 'static {
        stream::iter(
            indices.map(move |index| Ok(self.cases[case_index].fixture.replies[index].clone())),
        )
    }

    pub async fn serve(
        self: Arc<Self>,
        request: Request<Streaming<Bytes>>,
    ) -> Result<Response<Replies>, Status> {
        let ordinal = self.claim(request.remote_addr()).map_err(|_| reject())?;
        let step = &self.invocations[ordinal];
        let case_index = step.case_index;
        let case = &self.cases[case_index];
        let mut upload = Upload::new(request.into_inner());
        let init = upload.required().await.map_err(|_| reject())?;
        if init.as_ref() != step.initial.as_slice() {
            return Err(reject());
        }
        let split = case.fixture.expected.partials.len();
        let reply_count = case.fixture.replies.len();
        if split > 0 {
            upload.preview_prefix(case).await.map_err(|_| reject())?;
        }
        let previews = Arc::clone(&self).reply_range(case_index, 0..split);
        // No final or S3 END until the upload's half-close and all Ogg media
        // have been independently verified. On any error, never send the tail.
        let tail = stream::once(async move {
            let receipt = upload
                .finish(&self.cases[case_index])
                .await
                .map_err(|_| reject())?;
            self.record(ordinal, receipt).map_err(|_| reject())?;
            Ok::<_, Status>(self.reply_range(case_index, split..reply_count))
        })
        .try_flatten();
        Ok(replies(previews.chain(tail)))
    }
}

fn reject() -> Status {
    Status::failed_precondition("qualification upload rejected")
}

#[derive(Default)]
pub(super) struct UploadReceipt {
    pub requests: u64,
    pub request_bytes: u64,
    pub audio_messages: u64,
    pub ogg_bytes: u64,
}

struct Upload {
    stream: Streaming<Bytes>,
    ogg: Vec<u8>,
    elapsed: u64,
    receipt: UploadReceipt,
}

impl Upload {
    fn new(stream: Streaming<Bytes>) -> Self {
        Self {
            stream,
            ogg: Vec::new(),
            elapsed: 0,
            receipt: UploadReceipt::default(),
        }
    }

    async fn next(&mut self) -> Check<Option<Bytes>> {
        let next = self.stream.message().await.map_err(|_| Failed)?;
        if let Some(payload) = &next {
            check(self.receipt.requests < MAX_MESSAGES && payload.len() <= MAX_MESSAGE_BYTES)?;
            self.receipt.requests += 1;
            self.receipt.request_bytes += payload.len() as u64;
        }
        Ok(next)
    }

    async fn required(&mut self) -> Check<Bytes> {
        self.next().await?.ok_or(Failed)
    }

    fn audio(&mut self, payload: &[u8]) -> Check<()> {
        let fields = parse_fields(payload).map_err(|_| Failed)?;
        let [
            Field {
                number: AUDIO_INFO,
                value: Value::Bytes(info),
                ..
            },
            Field {
                number: AUDIO_DATA,
                value: Value::Bytes(data),
                ..
            },
        ] = fields.as_slice()
        else {
            return Err(Failed);
        };
        let info = parse_fields(info).map_err(|_| Failed)?;
        let data = parse_fields(data).map_err(|_| Failed)?;
        let (
            [
                Field {
                    number: 9,
                    value: Value::Varint(elapsed),
                    ..
                },
            ],
            [
                Field {
                    number: 1,
                    value: Value::Bytes(chunk),
                    ..
                },
            ],
        ) = (info.as_slice(), data.as_slice())
        else {
            return Err(Failed);
        };
        check(
            !chunk.is_empty()
                && *elapsed >= self.elapsed
                && u128::from(*elapsed) <= SESSION_DEADLINE.as_millis()
                && self.ogg.len() + chunk.len() <= MAX_OGG_BYTES
                && audio_request(chunk, *elapsed)
                    .map_err(|_| Failed)?
                    .as_slice()
                    == payload,
        )?;
        self.elapsed = *elapsed;
        self.ogg.extend_from_slice(chunk);
        self.receipt.audio_messages += 1;
        Ok(())
    }

    async fn preview_prefix(&mut self, case: &PreparedCase) -> Check<()> {
        // The client deliberately withholds the remaining PCM, allowing only the
        // two header packets and the first 20 ms media packet through the gate.
        while self.ogg.len() < case.first_media_end {
            let message = self.required().await?;
            self.audio(&message)?;
        }
        check(self.ogg.len() == case.first_media_end)?;
        let (packets, pages) = guarded(|| {
            (
                audio_common::packets(&self.ogg),
                audio_common::pages(&self.ogg),
            )
        })?;
        check(
            pages.len() == 3
                && pages[2].flags == 0
                && packets.get(2..) == case.codec_packets.get(..1),
        )
    }

    async fn finish(mut self, case: &PreparedCase) -> Check<UploadReceipt> {
        loop {
            let message = self.required().await?;
            if message.as_ref() == case.context.as_slice() {
                break;
            }
            self.audio(&message)?;
        }
        check(self.required().await?.as_ref() == end_request())?;
        check(self.next().await?.is_none())?;
        check(self.receipt.audio_messages > 0)?;
        // The existing common helper validates CRC/framing, serial/sequence,
        // unique EOS, pre-skip and trim, and decodes every packet independently.
        let packets = guarded(|| {
            audio_common::assert_duration(&self.ogg, case.fixture.pcm.len() / 2);
            audio_common::packets(&self.ogg)
        })?;
        check(packets.get(2..) == Some(case.codec_packets.as_slice()))?;
        self.receipt.ogg_bytes = self.ogg.len() as u64;
        Ok(self.receipt)
    }
}
