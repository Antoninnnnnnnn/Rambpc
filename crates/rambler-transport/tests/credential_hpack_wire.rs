//! S1 only: invented metadata through the native client, one IPv4 loopback TCP.
//! The h2 peer drives the protocol; it does NOT decide HPACK privacy acceptance.
//! No TLS, CONTINUATION, eviction, size updates, general HTTP/2 or live-service claim.
//! No capture, decoded field, or credential value is included in diagnostics.
use bytes::Bytes;
use futures_util::stream;
use rambler_transport::{
    CredentialValue, DEFAULT_METHOD, GrpcTransport, Metadata, MetadataFuture, MetadataProvider,
    StreamSource, TransportConfig,
};
use std::{
    collections::BTreeMap,
    io,
    pin::Pin,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc},
    task::JoinSet,
};

const CAP: usize = 65536;
const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const SENSITIVE: [&[u8]; 3] = [b"authorization", b"x-goog-api-key", b"proof-bin"];
// Fixed public arithmetic, NOT output computed through tonic or this oracle.
// 00ff7f -> AP9/; fbff01 -> +/8B; 000102 -> AAEC; 030405 -> AwQF.
const VALUES: [[&[u8]; 5]; 2] = [
    [
        b"invented-one-first",
        b"invented-one-second",
        b"invented-one-key",
        b"AP9/",
        b"+/8B",
    ],
    [
        b"invented-two-first",
        b"invented-two-second",
        b"invented-two-key",
        b"AAEC",
        b"AwQF",
    ],
];
fn sensitive(name: &[u8]) -> bool {
    SENSITIVE.contains(&name)
}
fn invented(value: &[u8]) -> bool {
    VALUES.iter().flatten().any(|v| *v == value)
}

type Pairs = BTreeMap<Vec<u8>, Vec<Vec<u8>>>;
fn append(map: &mut Pairs, name: &[u8], value: &[u8]) {
    map.entry(name.to_vec()).or_default().push(value.to_vec());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Privacy,
    Pollution,
    Binding,
    Framing,
    Huffman,
    Bounds,
    Index,
    Unsupported,
    Premise,
}
#[derive(Debug)]
struct Fault {
    kind: Kind,
    offset: usize,
    stream: u32,
    ordinal: usize,
}
type Check<T> = Result<T, Fault>;
fn fault(kind: Kind, offset: usize) -> Fault {
    Fault {
        kind,
        offset,
        stream: 0,
        ordinal: 0,
    }
}
fn ok<T>(result: Check<T>) -> T {
    match result {
        Ok(value) => value,
        Err(f) => panic!(
            "oracle {:?} offset={} stream={} ordinal={}",
            f.kind, f.offset, f.stream, f.ordinal
        ),
    }
}
fn rejects<T>(result: Check<T>, kind: Kind) {
    match result {
        Err(f) => assert!(f.kind == kind, "wrong oracle error class"),
        Ok(_) => panic!("oracle accepted negative fixture"),
    }
}

// RFC 7541 Appendix A, all 61 entries, transcribed from the isolated public RFC.
const STATIC: [(&str, &str); 61] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
    ("age", ""),
    ("allow", ""),
    ("authorization", ""),
    ("cache-control", ""),
    ("content-disposition", ""),
    ("content-encoding", ""),
    ("content-language", ""),
    ("content-length", ""),
    ("content-location", ""),
    ("content-range", ""),
    ("content-type", ""),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("expect", ""),
    ("expires", ""),
    ("from", ""),
    ("host", ""),
    ("if-match", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("if-range", ""),
    ("if-unmodified-since", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("max-forwards", ""),
    ("proxy-authenticate", ""),
    ("proxy-authorization", ""),
    ("range", ""),
    ("referer", ""),
    ("refresh", ""),
    ("retry-after", ""),
    ("server", ""),
    ("set-cookie", ""),
    ("strict-transport-security", ""),
    ("transfer-encoding", ""),
    ("user-agent", ""),
    ("vary", ""),
    ("via", ""),
    ("www-authenticate", ""),
];

// Standards DATA, not h2 implementation code: RFC 7541 Appendix B, symbols
// 0..=256 in order, (LSB-aligned code, bit length); EOS is entry 256.
// Public source read as data: rfc-editor.org/rfc/rfc7541.txt (May 2015).
const HUFFMAN: [(u32, u8); 257] = [
    // 0..15
    (0x1ff8, 13),
    (0x7fffd8, 23),
    (0xfffffe2, 28),
    (0xfffffe3, 28),
    (0xfffffe4, 28),
    (0xfffffe5, 28),
    (0xfffffe6, 28),
    (0xfffffe7, 28),
    (0xfffffe8, 28),
    (0xffffea, 24),
    (0x3ffffffc, 30),
    (0xfffffe9, 28),
    (0xfffffea, 28),
    (0x3ffffffd, 30),
    (0xfffffeb, 28),
    (0xfffffec, 28),
    // 16..31
    (0xfffffed, 28),
    (0xfffffee, 28),
    (0xfffffef, 28),
    (0xffffff0, 28),
    (0xffffff1, 28),
    (0xffffff2, 28),
    (0x3ffffffe, 30),
    (0xffffff3, 28),
    (0xffffff4, 28),
    (0xffffff5, 28),
    (0xffffff6, 28),
    (0xffffff7, 28),
    (0xffffff8, 28),
    (0xffffff9, 28),
    (0xffffffa, 28),
    (0xffffffb, 28),
    // 32..47
    (0x14, 6),
    (0x3f8, 10),
    (0x3f9, 10),
    (0xffa, 12),
    (0x1ff9, 13),
    (0x15, 6),
    (0xf8, 8),
    (0x7fa, 11),
    (0x3fa, 10),
    (0x3fb, 10),
    (0xf9, 8),
    (0x7fb, 11),
    (0xfa, 8),
    (0x16, 6),
    (0x17, 6),
    (0x18, 6),
    // 48..63
    (0x0, 5),
    (0x1, 5),
    (0x2, 5),
    (0x19, 6),
    (0x1a, 6),
    (0x1b, 6),
    (0x1c, 6),
    (0x1d, 6),
    (0x1e, 6),
    (0x1f, 6),
    (0x5c, 7),
    (0xfb, 8),
    (0x7ffc, 15),
    (0x20, 6),
    (0xffb, 12),
    (0x3fc, 10),
    // 64..79
    (0x1ffa, 13),
    (0x21, 6),
    (0x5d, 7),
    (0x5e, 7),
    (0x5f, 7),
    (0x60, 7),
    (0x61, 7),
    (0x62, 7),
    (0x63, 7),
    (0x64, 7),
    (0x65, 7),
    (0x66, 7),
    (0x67, 7),
    (0x68, 7),
    (0x69, 7),
    (0x6a, 7),
    // 80..95
    (0x6b, 7),
    (0x6c, 7),
    (0x6d, 7),
    (0x6e, 7),
    (0x6f, 7),
    (0x70, 7),
    (0x71, 7),
    (0x72, 7),
    (0xfc, 8),
    (0x73, 7),
    (0xfd, 8),
    (0x1ffb, 13),
    (0x7fff0, 19),
    (0x1ffc, 13),
    (0x3ffc, 14),
    (0x22, 6),
    // 96..111
    (0x7ffd, 15),
    (0x3, 5),
    (0x23, 6),
    (0x4, 5),
    (0x24, 6),
    (0x5, 5),
    (0x25, 6),
    (0x26, 6),
    (0x27, 6),
    (0x6, 5),
    (0x74, 7),
    (0x75, 7),
    (0x28, 6),
    (0x29, 6),
    (0x2a, 6),
    (0x7, 5),
    // 112..127
    (0x2b, 6),
    (0x76, 7),
    (0x2c, 6),
    (0x8, 5),
    (0x9, 5),
    (0x2d, 6),
    (0x77, 7),
    (0x78, 7),
    (0x79, 7),
    (0x7a, 7),
    (0x7b, 7),
    (0x7ffe, 15),
    (0x7fc, 11),
    (0x3ffd, 14),
    (0x1ffd, 13),
    (0xffffffc, 28),
    // 128..143
    (0xfffe6, 20),
    (0x3fffd2, 22),
    (0xfffe7, 20),
    (0xfffe8, 20),
    (0x3fffd3, 22),
    (0x3fffd4, 22),
    (0x3fffd5, 22),
    (0x7fffd9, 23),
    (0x3fffd6, 22),
    (0x7fffda, 23),
    (0x7fffdb, 23),
    (0x7fffdc, 23),
    (0x7fffdd, 23),
    (0x7fffde, 23),
    (0xffffeb, 24),
    (0x7fffdf, 23),
    // 144..159
    (0xffffec, 24),
    (0xffffed, 24),
    (0x3fffd7, 22),
    (0x7fffe0, 23),
    (0xffffee, 24),
    (0x7fffe1, 23),
    (0x7fffe2, 23),
    (0x7fffe3, 23),
    (0x7fffe4, 23),
    (0x1fffdc, 21),
    (0x3fffd8, 22),
    (0x7fffe5, 23),
    (0x3fffd9, 22),
    (0x7fffe6, 23),
    (0x7fffe7, 23),
    (0xffffef, 24),
    // 160..175
    (0x3fffda, 22),
    (0x1fffdd, 21),
    (0xfffe9, 20),
    (0x3fffdb, 22),
    (0x3fffdc, 22),
    (0x7fffe8, 23),
    (0x7fffe9, 23),
    (0x1fffde, 21),
    (0x7fffea, 23),
    (0x3fffdd, 22),
    (0x3fffde, 22),
    (0xfffff0, 24),
    (0x1fffdf, 21),
    (0x3fffdf, 22),
    (0x7fffeb, 23),
    (0x7fffec, 23),
    // 176..191
    (0x1fffe0, 21),
    (0x1fffe1, 21),
    (0x3fffe0, 22),
    (0x1fffe2, 21),
    (0x7fffed, 23),
    (0x3fffe1, 22),
    (0x7fffee, 23),
    (0x7fffef, 23),
    (0xfffea, 20),
    (0x3fffe2, 22),
    (0x3fffe3, 22),
    (0x3fffe4, 22),
    (0x7ffff0, 23),
    (0x3fffe5, 22),
    (0x3fffe6, 22),
    (0x7ffff1, 23),
    // 192..207
    (0x3ffffe0, 26),
    (0x3ffffe1, 26),
    (0xfffeb, 20),
    (0x7fff1, 19),
    (0x3fffe7, 22),
    (0x7ffff2, 23),
    (0x3fffe8, 22),
    (0x1ffffec, 25),
    (0x3ffffe2, 26),
    (0x3ffffe3, 26),
    (0x3ffffe4, 26),
    (0x7ffffde, 27),
    (0x7ffffdf, 27),
    (0x3ffffe5, 26),
    (0xfffff1, 24),
    (0x1ffffed, 25),
    // 208..223
    (0x7fff2, 19),
    (0x1fffe3, 21),
    (0x3ffffe6, 26),
    (0x7ffffe0, 27),
    (0x7ffffe1, 27),
    (0x3ffffe7, 26),
    (0x7ffffe2, 27),
    (0xfffff2, 24),
    (0x1fffe4, 21),
    (0x1fffe5, 21),
    (0x3ffffe8, 26),
    (0x3ffffe9, 26),
    (0xffffffd, 28),
    (0x7ffffe3, 27),
    (0x7ffffe4, 27),
    (0x7ffffe5, 27),
    // 224..239
    (0xfffec, 20),
    (0xfffff3, 24),
    (0xfffed, 20),
    (0x1fffe6, 21),
    (0x3fffe9, 22),
    (0x1fffe7, 21),
    (0x1fffe8, 21),
    (0x7ffff3, 23),
    (0x3fffea, 22),
    (0x3fffeb, 22),
    (0x1ffffee, 25),
    (0x1ffffef, 25),
    (0xfffff4, 24),
    (0xfffff5, 24),
    (0x3ffffea, 26),
    (0x7ffff4, 23),
    // 240..255, then EOS
    (0x3ffffeb, 26),
    (0x7ffffe6, 27),
    (0x3ffffec, 26),
    (0x3ffffed, 26),
    (0x7ffffe7, 27),
    (0x7ffffe8, 27),
    (0x7ffffe9, 27),
    (0x7ffffea, 27),
    (0x7ffffeb, 27),
    (0xffffffe, 28),
    (0x7ffffec, 27),
    (0x7ffffed, 27),
    (0x7ffffee, 27),
    (0x7ffffef, 27),
    (0x7fffff0, 27),
    (0x3ffffee, 26),
    (0x3fffffff, 30),
];
#[derive(Default)]
struct Node {
    child: [Option<usize>; 2],
    symbol: Option<usize>,
}
fn trie() -> &'static [Node] {
    static TREE: OnceLock<Vec<Node>> = OnceLock::new();
    TREE.get_or_init(|| {
        let mut nodes = vec![Node::default()];
        for (symbol, &(code, len)) in HUFFMAN.iter().enumerate() {
            assert!(
                (5..=30).contains(&len) && code < (1u32 << len),
                "RFC table range"
            );
            let mut node = 0;
            for shift in (0..len).rev() {
                assert!(nodes[node].symbol.is_none(), "RFC prefix conflict");
                let bit = ((code >> shift) & 1) as usize;
                let next = match nodes[node].child[bit] {
                    Some(next) => next,
                    None => {
                        let next = nodes.len();
                        nodes.push(Node::default());
                        nodes[node].child[bit] = Some(next);
                        next
                    }
                };
                node = next;
            }
            assert!(
                nodes[node].symbol.is_none() && nodes[node].child == [None, None],
                "RFC duplicate/prefix"
            );
            nodes[node].symbol = Some(symbol);
        }
        assert!(nodes.len() <= 513, "RFC tree bound");
        nodes
    })
}
fn unhuff(input: &[u8], offset: usize) -> Check<Vec<u8>> {
    let nodes = trie();
    let (mut node, mut pending, mut ones) = (0, 0usize, true);
    let mut output = Vec::new();
    for (i, &byte) in input.iter().enumerate() {
        for shift in (0..8).rev() {
            let bit = ((byte >> shift) & 1) as usize;
            node = nodes[node].child[bit].ok_or_else(|| fault(Kind::Huffman, offset + i))?;
            pending += 1;
            ones &= bit == 1;
            if let Some(symbol) = nodes[node].symbol {
                if symbol == 256 {
                    return Err(fault(Kind::Huffman, offset + i));
                }
                if output.len() == CAP {
                    return Err(fault(Kind::Bounds, offset + i));
                }
                output.push(symbol as u8);
                node = 0;
                pending = 0;
                ones = true;
            }
        }
    }
    if pending > 7 || !ones {
        return Err(fault(Kind::Huffman, offset + input.len()));
    }
    Ok(output)
}
struct Cursor<'a> {
    input: &'a [u8],
    pos: usize,
    base: usize,
}
impl Cursor<'_> {
    fn byte(&mut self) -> Check<u8> {
        let byte = *self
            .input
            .get(self.pos)
            .ok_or_else(|| fault(Kind::Framing, self.base + self.pos))?;
        self.pos += 1;
        Ok(byte)
    }
    fn integer(&mut self, bits: u8) -> Check<usize> {
        let mask = (1usize << bits) - 1;
        let mut value = usize::from(self.byte()?) & mask;
        if value < mask {
            return Ok(value);
        }
        for continuation in 0..4 {
            let byte = self.byte()?;
            value += usize::from(byte & 127) << (7 * continuation);
            if value > (1 << 20) {
                return Err(fault(Kind::Bounds, self.base + self.pos - 1));
            }
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        Err(fault(Kind::Bounds, self.base + self.pos))
    }
    fn string(&mut self) -> Check<(Vec<u8>, bool)> {
        let h = self.input.get(self.pos).is_some_and(|byte| byte & 128 != 0);
        let len = self.integer(7)?;
        if len > CAP {
            return Err(fault(Kind::Bounds, self.base + self.pos));
        }
        let end = self.pos + len;
        let bytes = self
            .input
            .get(self.pos..end)
            .ok_or_else(|| fault(Kind::Framing, self.base + self.pos))?;
        let value = if h {
            unhuff(bytes, self.base + self.pos)?
        } else {
            bytes.to_vec()
        };
        self.pos = end;
        Ok((value, h))
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Representation {
    Indexed,
    Incremental,
    Without,
    Never,
}
// Intentionally no Debug on any type containing decoded fields or raw capture.
struct Field {
    name: Vec<u8>,
    value: Vec<u8>,
    representation: Representation,
    offset: usize,
    index_end: usize,
    name_index: usize,
    huffman: bool,
}
#[derive(Default)]
struct Exercises {
    insertion: bool,
    dynamic_reuse: bool,
    without: bool,
    huffman_never: bool,
    static_sensitive: bool,
    literal_sensitive: bool,
}
impl Exercises {
    fn require(&self) -> Check<()> {
        if self.insertion
            && self.dynamic_reuse
            && self.without
            && self.huffman_never
            && self.static_sensitive
            && self.literal_sensitive
        {
            Ok(())
        } else {
            Err(fault(Kind::Premise, 0))
        }
    }
}
#[derive(Default)]
struct Decoder {
    table: Vec<(Vec<u8>, Vec<u8>)>,
    bytes: usize,
    exercises: Exercises,
}
impl Decoder {
    fn lookup(&self, index: usize, offset: usize) -> Check<(Vec<u8>, Vec<u8>)> {
        if (1..=61).contains(&index) {
            let (name, value) = STATIC[index - 1];
            return Ok((name.as_bytes().to_vec(), value.as_bytes().to_vec()));
        }
        if index >= 62
            && let Some(pair) = self.table.get(index - 62)
        {
            return Ok(pair.clone());
        }
        Err(fault(Kind::Index, offset))
    }
    fn field(&mut self, c: &mut Cursor<'_>) -> Check<Field> {
        let offset = c.base + c.pos;
        let first = *c
            .input
            .get(c.pos)
            .ok_or_else(|| fault(Kind::Framing, offset))?;
        let representation = if first & 128 != 0 {
            Representation::Indexed
        } else if first & 64 != 0 {
            Representation::Incremental
        } else if first & 32 != 0 {
            return Err(fault(Kind::Unsupported, offset));
        } else if first & 16 != 0 {
            Representation::Never
        } else {
            Representation::Without
        };
        let bits = match representation {
            Representation::Indexed => 7,
            Representation::Incremental => 6,
            _ => 4,
        };
        let index = c.integer(bits)?;
        let index_end = c.base + c.pos;
        let (name, indexed_value) = if representation == Representation::Indexed || index != 0 {
            self.lookup(index, offset)?
        } else {
            (c.string()?.0, Vec::new())
        };
        // Stop at THIS field, before any insertion can shift later dynamic indices.
        if sensitive(&name) {
            if representation == Representation::Incremental {
                return Err(fault(Kind::Pollution, offset));
            }
            if representation != Representation::Never {
                return Err(fault(Kind::Privacy, offset));
            }
        }
        let (value, huffman) = if representation == Representation::Indexed {
            (indexed_value, false)
        } else {
            c.string()?
        };
        if invented(&value) && representation == Representation::Incremental {
            return Err(fault(Kind::Pollution, offset));
        }
        if invented(&value) && !sensitive(&name) {
            return Err(fault(Kind::Binding, offset));
        }
        match representation {
            Representation::Incremental => {
                let bytes = name.len() + value.len() + 32;
                if self.table.len() == 32 || self.bytes + bytes > 4096 {
                    return Err(fault(Kind::Bounds, offset));
                }
                self.bytes += bytes;
                self.table.insert(0, (name.clone(), value.clone()));
                self.exercises.insertion = true;
            }
            Representation::Indexed => self.exercises.dynamic_reuse |= index >= 62,
            Representation::Without => self.exercises.without = true,
            Representation::Never => {
                self.exercises.huffman_never |= sensitive(&name) && huffman;
                self.exercises.static_sensitive |= sensitive(&name) && index == 23;
                self.exercises.literal_sensitive |= sensitive(&name) && index == 0;
            }
        }
        Ok(Field {
            name,
            value,
            representation,
            offset,
            index_end,
            name_index: index,
            huffman,
        })
    }
    fn block(&mut self, bytes: &[u8], base: usize, stream: u32) -> Check<Vec<Field>> {
        if bytes.len() > CAP {
            return Err(fault(Kind::Bounds, base));
        }
        let mut c = Cursor {
            input: bytes,
            pos: 0,
            base,
        };
        let mut fields = Vec::new();
        while c.pos < bytes.len() {
            if fields.len() == 64 {
                return Err(fault(Kind::Bounds, base + c.pos));
            }
            let field = self.field(&mut c).map_err(|mut f| {
                f.stream = stream;
                f.ordinal = fields.len();
                f
            })?;
            fields.push(field);
        }
        Ok(fields)
    }
}

fn expected_credentials(map: &mut Pairs, generation: usize) {
    let v = VALUES[generation];
    append(map, SENSITIVE[0], v[0]);
    append(map, SENSITIVE[0], v[1]);
    append(map, SENSITIVE[1], v[2]);
    append(map, SENSITIVE[2], v[3]);
    append(map, SENSITIVE[2], v[4]);
}
fn regulars(fields: &[Field]) -> Pairs {
    let mut map = Pairs::new();
    for f in fields.iter().filter(|f| !f.name.starts_with(b":")) {
        append(&mut map, &f.name, &f.value);
    }
    map
}
fn binding(fields: &[Field], stream: u32, authority: &[u8]) -> Check<()> {
    let bad = || Fault {
        kind: Kind::Binding,
        offset: fields.first().map_or(0, |f| f.offset),
        stream,
        ordinal: 0,
    };
    let mut pseudos = Pairs::new();
    let mut regular_seen = false;
    for f in fields {
        if f.name.starts_with(b":") {
            if regular_seen {
                return Err(bad());
            }
            append(&mut pseudos, &f.name, &f.value);
        } else {
            regular_seen = true;
        }
        if sensitive(&f.name) && f.representation != Representation::Never {
            return Err(fault(Kind::Privacy, f.offset));
        }
    }
    let mut expected = Pairs::new();
    for (name, value) in [
        (b":method".as_slice(), b"POST".as_slice()),
        (b":scheme", b"http"),
        (b":authority", authority),
        (b":path", DEFAULT_METHOD.as_bytes()),
    ] {
        append(&mut expected, name, value);
    }
    if pseudos != expected {
        return Err(bad());
    }
    let mut regular = regulars(fields);
    let timeouts = regular.remove(b"grpc-timeout".as_slice()).ok_or_else(bad)?;
    if timeouts.len() != 1 {
        return Err(bad());
    }
    let t = &timeouts[0];
    if !(2..=9).contains(&t.len())
        || !t[..t.len() - 1].iter().all(u8::is_ascii_digit)
        || !b"HMSmun".contains(&t[t.len() - 1])
    {
        return Err(bad());
    }
    expected.clear();
    append(&mut expected, b"content-type", b"application/grpc");
    append(&mut expected, b"te", b"trailers");
    match stream {
        1 => expected_credentials(&mut expected, 0),
        3 => expected_credentials(&mut expected, 1),
        5 => {}
        _ => return Err(bad()),
    }
    if regular != expected {
        return Err(bad());
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Terminal {
    Missing,
    Eof,
    Reset,
    Aborted,
    Other,
}
struct Capture {
    bytes: Vec<u8>,
    overflow: bool,
    terminal: Terminal,
}
impl Default for Capture {
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            overflow: false,
            terminal: Terminal::Missing,
        }
    }
}
struct Tee {
    socket: TcpStream,
    capture: Arc<Mutex<Capture>>,
}
impl AsyncRead for Tee {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let room = buf.remaining();
        let result = Pin::new(&mut this.socket).poll_read(cx, buf);
        if let Poll::Ready(ref result) = result {
            let mut capture = this.capture.lock().unwrap();
            match result {
                Ok(()) => {
                    let newly_read = &buf.filled()[before..];
                    if newly_read.is_empty() && room != 0 {
                        capture.terminal = Terminal::Eof;
                    }
                    if newly_read.len() > CAP - capture.bytes.len() {
                        capture.overflow = true;
                        return Poll::Ready(Err(io::Error::other("capture bound")));
                    }
                    capture.bytes.extend_from_slice(newly_read);
                }
                Err(e) => {
                    capture.terminal = match e.kind() {
                        io::ErrorKind::ConnectionReset => Terminal::Reset,
                        io::ErrorKind::ConnectionAborted => Terminal::Aborted,
                        _ => Terminal::Other,
                    }
                }
            }
        }
        result
    }
}
impl AsyncWrite for Tee {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().socket).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().socket).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().socket).poll_shutdown(cx)
    }
}
struct Block {
    frame: usize,
    stream: u32,
    fields: Vec<Field>,
}
struct Trace {
    blocks: Vec<Block>,
    upload_end: usize,
    decoder: Decoder,
}
fn parse(capture: &Capture, authority: &[u8]) -> Check<Trace> {
    let bytes = &capture.bytes;
    if capture.overflow || bytes.len() > CAP {
        return Err(fault(Kind::Bounds, 0));
    }
    if !matches!(
        capture.terminal,
        Terminal::Eof | Terminal::Reset | Terminal::Aborted
    ) {
        return Err(fault(Kind::Framing, bytes.len()));
    }
    if !bytes.starts_with(PREFACE) {
        return Err(fault(Kind::Framing, 0));
    }
    let mut pos = PREFACE.len();
    let mut decoder = Decoder::default();
    let mut blocks = Vec::new();
    let (mut initial_settings, mut ack, mut windows, mut goaway) = (false, false, 0, false);
    let mut uploaded = [false; 3];
    let mut upload_end = 0;
    while pos < bytes.len() {
        let frame = pos;
        let h = bytes
            .get(pos..pos + 9)
            .ok_or_else(|| fault(Kind::Framing, pos))?;
        let size = (usize::from(h[0]) << 16) | (usize::from(h[1]) << 8) | usize::from(h[2]);
        if size > 16384 {
            return Err(fault(Kind::Bounds, pos));
        }
        let stream = u32::from_be_bytes(h[5..9].try_into().unwrap());
        let (kind, flags) = (h[3], h[4]);
        pos += 9;
        let payload = bytes
            .get(pos..pos + size)
            .ok_or_else(|| fault(Kind::Framing, frame))?;
        if goaway || stream & 0x80000000 != 0 || (!initial_settings && (kind != 4 || flags != 0)) {
            return Err(fault(Kind::Framing, frame));
        }
        match kind {
            4 => {
                if stream != 0 {
                    return Err(fault(Kind::Framing, frame));
                }
                match flags {
                    0 if !initial_settings && size.is_multiple_of(6) && size <= 96 => {
                        initial_settings = true
                    }
                    1 if initial_settings && !ack && size == 0 => ack = true,
                    _ => return Err(fault(Kind::Framing, frame)),
                }
            }
            8 => {
                windows += 1;
                if windows > 8
                    || stream != 0
                    || flags != 0
                    || size != 4
                    || u32::from_be_bytes(payload.try_into().unwrap()) & 0x7fffffff == 0
                {
                    return Err(fault(Kind::Framing, frame));
                }
            }
            1 => {
                if flags != 4 || blocks.len() == 3 || stream != [1, 3, 5][blocks.len()] {
                    return Err(fault(Kind::Framing, frame));
                }
                let fields = decoder.block(payload, pos, stream)?;
                binding(&fields, stream, authority)?;
                blocks.push(Block {
                    frame,
                    stream,
                    fields,
                });
            }
            0 => {
                let Some(slot) = [1, 3, 5].iter().position(|&s| s == stream) else {
                    return Err(fault(Kind::Framing, frame));
                };
                if flags != 1 || size != 0 || slot >= blocks.len() || uploaded[slot] {
                    return Err(fault(Kind::Framing, frame));
                }
                uploaded[slot] = true;
                upload_end = pos + size;
            }
            7 => {
                if stream != 0
                    || flags != 0
                    || size != 8
                    || payload != [0; 8]
                    || uploaded != [true; 3]
                {
                    return Err(fault(Kind::Framing, frame));
                }
                goaway = true;
            }
            3 => return Err(fault(Kind::Premise, frame)), // RST_STREAM is NOT a privacy finding.
            _ => return Err(fault(Kind::Framing, frame)), // Includes CONTINUATION: S2 deferred.
        }
        pos += size;
    }
    if !initial_settings || blocks.len() != 3 || uploaded != [true; 3] {
        return Err(fault(Kind::Framing, pos));
    }
    Ok(Trace {
        blocks,
        upload_end,
        decoder,
    })
}

struct RotatingProvider {
    current: Mutex<Metadata>,
    calls: AtomicUsize,
}
impl MetadataProvider for RotatingProvider {
    fn metadata(&self) -> MetadataFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let snapshot = self.current.lock().unwrap().clone();
        Box::pin(async move { Ok(snapshot) })
    }
}
fn metadata(generation: usize) -> Metadata {
    let v = VALUES[generation];
    let binaries: [[u8; 3]; 2] = if generation == 0 {
        [[0, 255, 127], [251, 255, 1]]
    } else {
        [[0, 1, 2], [3, 4, 5]]
    };
    let text = |value: &[u8]| match String::from_utf8(value.to_vec()) {
        Ok(value) => CredentialValue::Text(value),
        Err(_) => panic!("synthetic ASCII fixture"),
    };
    // No values are formatted in a failure, including constructor errors.
    match Metadata::new([
        ("authorization".into(), text(v[0])),
        ("authorization".into(), text(v[1])),
        ("x-goog-api-key".into(), text(v[2])),
        (
            "proof-bin".into(),
            CredentialValue::Binary(Bytes::copy_from_slice(&binaries[0])),
        ),
        (
            "proof-bin".into(),
            CredentialValue::Binary(Bytes::copy_from_slice(&binaries[1])),
        ),
    ]) {
        Ok(metadata) => metadata,
        Err(_) => panic!("synthetic metadata construction"),
    }
}
fn peer_pairs(headers: &http::HeaderMap) -> Pairs {
    let mut map = Pairs::new();
    for name in headers.keys() {
        for value in headers.get_all(name).iter() {
            append(&mut map, name.as_str().as_bytes(), value.as_bytes());
        }
    }
    map
}
fn terminal_error(
    kind: Option<io::ErrorKind>,
    capture: &Capture,
    completed: bool,
    uploads: usize,
) -> bool {
    completed
        && uploads == 3
        && !capture.overflow
        && matches!(
            (kind, capture.terminal),
            (Some(io::ErrorKind::ConnectionReset), Terminal::Reset)
                | (Some(io::ErrorKind::ConnectionAborted), Terminal::Aborted)
        )
}

#[tokio::test]
async fn native_metadata_never_indexed_across_three_sessions() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("owned IPv4 listener");
        let address = listener.local_addr().expect("owned address");
        assert!(
            address.is_ipv4() && address.ip().is_loopback(),
            "owned IPv4 only"
        );
        let authority = address.to_string().into_bytes();
        let capture = Arc::new(Mutex::new(Capture::default()));
        let completed = Arc::new(AtomicBool::new(false));
        let uploads = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Semaphore::new(0));
        let (seen_tx, mut seen_rx) = mpsc::channel(3);
        let server_capture = capture.clone();
        let server_completed = completed.clone();
        let server_uploads = uploads.clone();
        let server_release = release.clone();
        // Neither server nor second-accept observer is a detached task. Dropping
        // the outer join on timeout drops TCP/connection and aborts the JoinSet.
        let server = async move {
            let (socket, _) = listener.accept().await.expect("first owned accept");
            let serve = async {
                let tee = Tee {
                    socket,
                    capture: server_capture.clone(),
                };
                let mut connection = match h2::server::handshake(tee).await {
                    Ok(connection) => connection,
                    Err(_) => panic!("owned handshake failed"),
                };
                let mut handlers = JoinSet::new();
                let mut requests = 0;
                loop {
                    match connection.accept().await {
                        Some(Ok((request, mut respond))) => {
                            requests += 1;
                            assert!(requests <= 3, "unexpected RPC/replay");
                            assert!(
                                request.method() == http::Method::POST
                                    && request.uri().path() == DEFAULT_METHOD,
                                "peer request shape"
                            );
                            assert!(
                                seen_tx.send(request.headers().clone()).await.is_ok(),
                                "peer observation channel"
                            );
                            let release = server_release.clone();
                            let uploads = server_uploads.clone();
                            handlers.spawn(async move {
                                let mut body = request.into_body();
                                while let Some(chunk) = body.data().await {
                                    let chunk = match chunk {
                                        Ok(chunk) => chunk,
                                        Err(_) => panic!("owned upload read"),
                                    };
                                    assert!(chunk.is_empty(), "synthetic upload must be empty");
                                    assert!(
                                        body.flow_control().release_capacity(chunk.len()).is_ok(),
                                        "owned flow release"
                                    );
                                }
                                assert!(
                                    matches!(body.trailers().await, Ok(None)),
                                    "owned upload trailers"
                                );
                                uploads.fetch_add(1, Ordering::SeqCst);
                                release.acquire().await.expect("response gate").forget();
                                let response = http::Response::builder()
                                    .header("content-type", "application/grpc")
                                    .body(())
                                    .expect("synthetic response");
                                let mut stream = match respond.send_response(response, false) {
                                    Ok(stream) => stream,
                                    Err(_) => panic!("owned response send"),
                                };
                                let mut trailers = http::HeaderMap::new();
                                trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                                assert!(
                                    stream.send_trailers(trailers).is_ok(),
                                    "owned trailer send"
                                );
                            });
                        }
                        Some(Err(error)) => {
                            let capture = server_capture.lock().unwrap();
                            assert!(
                                terminal_error(
                                    error.get_io().map(io::Error::kind),
                                    &capture,
                                    server_completed.load(Ordering::SeqCst),
                                    server_uploads.load(Ordering::SeqCst)
                                ),
                                "unexpected peer terminal error"
                            );
                            break;
                        }
                        None => break,
                    }
                }
                while let Some(joined) = handlers.join_next().await {
                    assert!(joined.is_ok(), "owned handler failed");
                }
                assert!(
                    requests == 3 && server_uploads.load(Ordering::SeqCst) == 3,
                    "peer counts"
                );
                requests
            };
            tokio::pin!(serve);
            tokio::select! {
                biased;
                extra = listener.accept() => {
                    assert!(extra.is_ok(), "second-accept observer I/O failure");
                    panic!("extra TCP connection/reconnect");
                }
                requests = &mut serve => requests,
            }
        };
        let client = async {
            let provider = Arc::new(RotatingProvider {
                current: Mutex::new(Metadata::default()),
                calls: AtomicUsize::new(0),
            });
            let config = TransportConfig::loopback(address).expect("owned endpoint config");
            let transport = match GrpcTransport::connect(config).await {
                Ok(transport) => transport,
                Err(_) => panic!("owned client connect"),
            };
            let mut sessions = Vec::new();
            let mut peer = Vec::new();
            for generation in [Some(0), Some(1), None] {
                *provider.current.lock().unwrap() = generation.map(metadata).unwrap_or_default();
                let session = match transport.start_with_provider(
                    StreamSource::new(stream::empty()),
                    provider.clone(),
                    Duration::from_secs(5),
                ) {
                    Ok(session) => session,
                    Err(_) => panic!("owned session start"),
                };
                sessions.push(session);
                peer.push(peer_pairs(
                    &seen_rx.recv().await.expect("peer HEADERS observation"),
                ));
                assert!(
                    provider.calls.load(Ordering::SeqCst) == sessions.len(),
                    "provider snapshot count"
                );
                assert!(
                    sessions
                        .iter()
                        .all(|s| s.completion().terminal_code.is_none()),
                    "sessions must overlap"
                );
            }
            release.add_permits(3);
            for session in &mut sessions {
                assert!(
                    matches!(session.next().await, Ok(None)),
                    "owned session response"
                );
                let completion = match session.finish().await {
                    Ok(completion) => completion,
                    Err(_) => panic!("owned session completion"),
                };
                assert!(
                    completion.source_exhausted
                        && completion.source_closed
                        && completion.upload_eof_polled,
                    "source cleanup/upload EOF"
                );
                assert!(
                    completion.requests_produced == 0
                        && completion.terminal_code == Some(tonic::Code::Ok),
                    "empty successful completion"
                );
            }
            assert!(
                provider.calls.load(Ordering::SeqCst) == 3,
                "exactly three provider calls"
            );
            completed.store(true, Ordering::SeqCst);
            // Before awaiting server termination; both closes must succeed.
            assert!(transport.close().await.is_ok(), "first close");
            assert!(transport.close().await.is_ok(), "repeated close");
            peer
        };
        let (requests, peer) = tokio::join!(server, client);
        assert!(
            requests == 3 && completed.load(Ordering::SeqCst),
            "completed peer lifecycle"
        );
        let capture = capture.lock().unwrap();
        let trace = ok(parse(&capture, &authority));
        ok(trace.decoder.exercises.require());
        assert!(
            trace.blocks.len() == peer.len(),
            "independent HEADERS count"
        );
        for (block, headers) in trace.blocks.iter().zip(&peer) {
            assert!(
                regulars(&block.fields) == *headers,
                "auxiliary h2 decoded cross-check"
            );
        }
        mutate_real_capture(&capture, &authority, &trace);
    })
    .await
    .expect("bounded native HPACK privacy regression");
}

// A tiny fixture builder, NOT a second production encoder. Raw literal fixture
// strings and RFC hex strings below are independent of the native send path.
fn integer(out: &mut Vec<u8>, high: u8, bits: u8, mut value: usize) {
    let max = (1usize << bits) - 1;
    if value < max {
        out.push(high | value as u8);
        return;
    }
    out.push(high | max as u8);
    value -= max;
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
fn raw_string(out: &mut Vec<u8>, value: &[u8]) {
    integer(out, 0, 7, value.len());
    out.extend_from_slice(value);
}
fn literal(out: &mut Vec<u8>, representation: u8, index: usize, name: &[u8], value: &[u8]) {
    integer(
        out,
        representation,
        if representation == 0x40 { 6 } else { 4 },
        index,
    );
    if index == 0 {
        raw_string(out, name);
    }
    raw_string(out, value);
}
fn hex(input: &str) -> Vec<u8> {
    let compact: Vec<_> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    assert!(compact.len().is_multiple_of(2), "RFC hex length");
    compact
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            let digit = |b: u8| match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                _ => panic!("RFC hex digit"),
            };
            digit(p[0]) * 16 + digit(p[1])
        })
        .collect()
}
fn frame(out: &mut Vec<u8>, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
    let len = payload.len();
    out.extend_from_slice(&[(len >> 16) as u8, (len >> 8) as u8, len as u8, kind, flags]);
    out.extend_from_slice(&stream.to_be_bytes());
    out.extend_from_slice(payload);
}
fn fixture_block(stream: u32) -> Vec<u8> {
    let mut block = vec![0x83, 0x86];
    literal(&mut block, 0, 1, b"", b"127.0.0.1:1");
    literal(&mut block, 0, 4, b"", DEFAULT_METHOD.as_bytes());
    literal(&mut block, 0, 31, b"", b"application/grpc");
    literal(&mut block, 0, 0, b"te", b"trailers");
    literal(&mut block, 0, 0, b"grpc-timeout", b"5S");
    if stream != 5 {
        let v = VALUES[if stream == 1 { 0 } else { 1 }];
        literal(&mut block, 0x10, 23, b"", v[0]);
        literal(&mut block, 0x10, 23, b"", v[1]);
        literal(&mut block, 0x10, 0, SENSITIVE[1], v[2]);
        literal(&mut block, 0x10, 0, SENSITIVE[2], v[3]);
        literal(&mut block, 0x10, 0, SENSITIVE[2], v[4]);
    }
    block
}
fn fixture_capture() -> Capture {
    let mut bytes = PREFACE.to_vec();
    frame(&mut bytes, 4, 0, 0, &[]);
    for stream in [1, 3, 5] {
        frame(&mut bytes, 1, 4, stream, &fixture_block(stream));
        frame(&mut bytes, 0, 1, stream, &[]);
    }
    Capture {
        bytes,
        overflow: false,
        terminal: Terminal::Eof,
    }
}
fn mutate_real_capture(capture: &Capture, authority: &[u8], trace: &Trace) {
    let mut mutations = 0;
    for block in &trace.blocks {
        for (ordinal, field) in block
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| sensitive(&f.name))
        {
            assert!(
                field.representation == Representation::Never
                    && capture.bytes[field.offset] & 0xf0 == 0x10,
                "sensitive representation metadata"
            );
            let mut changed = Capture {
                bytes: capture.bytes.clone(),
                overflow: false,
                terminal: Terminal::Eof,
            };
            changed.bytes[field.offset] &= !0x10;
            let result = parse(&changed, authority);
            match result {
                Err(f) => assert!(
                    f.kind == Kind::Privacy
                        && f.offset == field.offset
                        && f.stream == block.stream
                        && f.ordinal == ordinal,
                    "real never-bit mutation must fail at this field"
                ),
                Ok(_) => panic!("real never-bit mutation accepted"),
            }
            let mut prefix = Vec::new();
            integer(&mut prefix, 0x40, 6, field.name_index);
            changed.bytes = capture.bytes.clone();
            let old = field.index_end - field.offset;
            let new = prefix.len();
            changed.bytes.splice(field.offset..field.index_end, prefix);
            let h = block.frame;
            let size = (usize::from(capture.bytes[h]) << 16)
                | (usize::from(capture.bytes[h + 1]) << 8)
                | usize::from(capture.bytes[h + 2]);
            let size = size + new - old;
            changed.bytes[h] = (size >> 16) as u8;
            changed.bytes[h + 1] = (size >> 8) as u8;
            changed.bytes[h + 2] = size as u8;
            match parse(&changed, authority) {
                Err(f) => assert!(
                    f.kind == Kind::Pollution
                        && f.offset == field.offset
                        && f.stream == block.stream
                        && f.ordinal == ordinal,
                    "real incremental mutation must fail before index shifts"
                ),
                Ok(_) => panic!("real incremental mutation accepted"),
            }
            mutations += 1;
        }
    }
    assert!(mutations == 10, "every real sensitive field mutated twice");
    // All prefixes before the final complete upload must fail, and every
    // mid-frame prefix must fail even after uploads (e.g. a partial GOAWAY).
    // Record boundaries only from the already accepted trace. SETTINGS ACK and
    // WINDOW_UPDATE may legally follow the uploads, not just optional GOAWAY.
    let mut boundaries = Vec::new();
    let mut pos = PREFACE.len();
    while pos < capture.bytes.len() {
        let h = &capture.bytes[pos..pos + 9];
        let size = (usize::from(h[0]) << 16) | (usize::from(h[1]) << 8) | usize::from(h[2]);
        pos += 9 + size;
        boundaries.push(pos);
    }
    for end in 0..capture.bytes.len() {
        let cut = Capture {
            bytes: capture.bytes[..end].to_vec(),
            overflow: false,
            terminal: Terminal::Eof,
        };
        if end < trace.upload_end || boundaries.binary_search(&end).is_err() {
            rejects(parse(&cut, authority), Kind::Framing);
        } else {
            ok(parse(&cut, authority));
        }
    }
}

fn decoded_equals(fields: &[Field], expected: &[(&[u8], &[u8])]) {
    assert!(fields.len() == expected.len(), "RFC decoded field count");
    for (f, (name, value)) in fields.iter().zip(expected) {
        assert!(f.name == *name && f.value == *value, "RFC decoded pair");
    }
}
#[test]
fn oracle_accepts_rfc_hpack_vectors_and_valid_edges() {
    assert!(
        STATIC.len() == 61 && HUFFMAN.len() == 257,
        "complete standards table counts"
    );
    let kraft: u64 = HUFFMAN.iter().map(|(_, len)| 1u64 << (30 - len)).sum();
    assert!(
        kraft == 1u64 << 30 && HUFFMAN[256] == (0x3fffffff, 30),
        "RFC Kraft/EOS"
    );
    // Canonical reconstruction by (length, symbol) must reproduce EVERY code.
    let mut sorted: Vec<_> = HUFFMAN
        .iter()
        .enumerate()
        .map(|(symbol, &(code, len))| (len, symbol, code))
        .collect();
    sorted.sort_unstable();
    let (mut next, mut previous) = (0u32, 0u8);
    for (len, _, code) in sorted {
        next <<= len - previous;
        assert!(next == code, "RFC canonical code");
        next += 1;
        previous = len;
    }
    let _ = trie(); // Independently verifies prefix-freeness and symbol uniqueness.
    for (encoded, bits, expected) in [
        (vec![10], 5, 10),
        (vec![31, 154, 10], 5, 1337),
        (vec![42], 8, 42),
    ] {
        let mut c = Cursor {
            input: &encoded,
            pos: 0,
            base: 0,
        };
        assert!(
            ok(c.integer(bits)) == expected && c.pos == encoded.len(),
            "RFC C.1 integer"
        );
    }
    for (encoded, name, value, representation, table_bytes) in [
        (
            "400a637573746f6d2d6b65790d637573746f6d2d686561646572",
            b"custom-key".as_slice(),
            b"custom-header".as_slice(),
            Representation::Incremental,
            55,
        ),
        (
            "040c2f73616d706c652f70617468",
            b":path",
            b"/sample/path",
            Representation::Without,
            0,
        ),
        (
            "100870617373776f726406736563726574",
            b"password",
            b"secret",
            Representation::Never,
            0,
        ),
        ("82", b":method", b"GET", Representation::Indexed, 0),
    ] {
        let mut d = Decoder::default();
        let fields = ok(d.block(&hex(encoded), 0, 0));
        decoded_equals(&fields, &[(name, value)]);
        assert!(
            fields[0].representation == representation && d.bytes == table_bytes,
            "RFC C.2 state/representation"
        );
    }
    // C.3 and C.4 are separate persistent decoding contexts, three requests each.
    for vectors in [
        [
            "828684410f7777772e6578616d706c652e636f6d",
            "828684be58086e6f2d6361636865",
            "828785bf400a637573746f6d2d6b65790c637573746f6d2d76616c7565",
        ],
        [
            "828684418cf1e3c2e5f23a6ba0ab90f4ff",
            "828684be5886a8eb10649cbf",
            "828785bf408825a849e95ba97d7f8925a849e95bb8e8b4bf",
        ],
    ] {
        let mut d = Decoder::default();
        for (request, encoded) in vectors.iter().enumerate() {
            let fields = ok(d.block(&hex(encoded), 0, 0));
            let mut expected: Vec<(&[u8], &[u8])> = vec![
                (b":method", b"GET"),
                (b":scheme", if request == 2 { b"https" } else { b"http" }),
                (b":path", if request == 2 { b"/index.html" } else { b"/" }),
                (b":authority", b"www.example.com"),
            ];
            if request == 1 {
                expected.push((b"cache-control", b"no-cache"));
            }
            if request == 2 {
                expected.push((b"custom-key", b"custom-value"));
            }
            decoded_equals(&fields, &expected);
            let mut table: Vec<(Vec<u8>, Vec<u8>)> =
                vec![(b":authority".to_vec(), b"www.example.com".to_vec())];
            if request >= 1 {
                table.insert(0, (b"cache-control".to_vec(), b"no-cache".to_vec()));
            }
            if request == 2 {
                table.insert(0, (b"custom-key".to_vec(), b"custom-value".to_vec()));
            }
            assert!(
                d.table == table && d.bytes == [57, 110, 164][request],
                "RFC C.3/C.4 exact dynamic state"
            );
        }
    }
    assert!(
        ok(unhuff(&hex("f1e3c2e5f23a6ba0ab90f4ff"), 0)) == b"www.example.com",
        "independent RFC Huffman string"
    );
    // A non-sensitive dynamic NAME is valid without permitting sensitive entries.
    let mut d = Decoder::default();
    let mut first = Vec::new();
    literal(&mut first, 0x40, 0, b"x-public", b"one");
    ok(d.block(&first, 0, 0));
    let mut second = Vec::new();
    literal(&mut second, 0x10, 62, b"", b"two");
    let fields = ok(d.block(&second, 0, 0));
    decoded_equals(&fields, &[(b"x-public", b"two")]);
    assert!(
        fields[0].name_index == 62 && !fields[0].huffman && d.table.len() == 1,
        "raw dynamic-name never literal"
    );
    let mut raw_sensitive = Vec::new();
    literal(&mut raw_sensitive, 0x10, 23, b"", VALUES[0][0]);
    let fields = ok(Decoder::default().block(&raw_sensitive, 0, 1));
    decoded_equals(&fields, &[(b"authorization", VALUES[0][0])]);
    assert!(
        !fields[0].huffman && fields[0].representation == Representation::Never,
        "raw sensitive positive"
    );
    let capture = fixture_capture();
    ok(parse(&capture, b"127.0.0.1:1"));
    // Synthetic raw trace deliberately does not qualify the native exercise guard.
    rejects(
        ok(parse(&capture, b"127.0.0.1:1"))
            .decoder
            .exercises
            .require(),
        Kind::Premise,
    );
}

#[test]
fn oracle_rejects_literal_privacy_and_binding_violations() {
    for (representation, index, name, kind) in [
        (0x40, 23, b"".as_slice(), Kind::Pollution),
        (0x40, 0, b"authorization", Kind::Pollution),
        (0, 23, b"", Kind::Privacy),
        (0, 0, b"authorization", Kind::Privacy),
    ] {
        let mut block = Vec::new();
        literal(&mut block, representation, index, name, VALUES[0][0]);
        rejects(Decoder::default().block(&block, 0, 1), kind);
    }
    rejects(Decoder::default().block(&[0x97], 0, 1), Kind::Privacy);
    // Insert-then-reference is rejected at insertion, not merely the later index.
    let mut pollution = Vec::new();
    literal(&mut pollution, 0x40, 0, b"proof-bin", VALUES[0][3]);
    pollution.push(0xbe);
    let mut d = Decoder::default();
    rejects(d.block(&pollution, 0, 1), Kind::Pollution);
    assert!(d.table.is_empty(), "pollution was not committed");
    let mut smuggled = Vec::new();
    literal(&mut smuggled, 0x10, 0, b"te", VALUES[0][0]);
    rejects(Decoder::default().block(&smuggled, 0, 1), Kind::Binding);
    let mut inserted_value = Vec::new();
    literal(&mut inserted_value, 0x40, 0, b"x-public", VALUES[0][0]);
    rejects(
        Decoder::default().block(&inserted_value, 0, 1),
        Kind::Pollution,
    );
    let one = ok(Decoder::default().block(&fixture_block(1), 0, 1));
    rejects(binding(&one, 3, b"127.0.0.1:1"), Kind::Binding); // Wrong generation.
    rejects(binding(&one, 5, b"127.0.0.1:1"), Kind::Binding); // Stale anonymous metadata.
    for name in SENSITIVE {
        let mut fields = ok(Decoder::default().block(&fixture_block(1), 0, 1));
        let at = fields.iter().position(|f| f.name == name).unwrap();
        fields.remove(at);
        rejects(binding(&fields, 1, b"127.0.0.1:1"), Kind::Binding);
        let duplicate = if name == SENSITIVE[0] {
            VALUES[0][0]
        } else if name == SENSITIVE[1] {
            VALUES[0][2]
        } else {
            VALUES[0][3]
        };
        let mut block = fixture_block(1);
        literal(&mut block, 0x10, 0, name, duplicate);
        rejects(
            binding(
                &ok(Decoder::default().block(&block, 0, 1)),
                1,
                b"127.0.0.1:1",
            ),
            Kind::Binding,
        );
    }
    for name in [SENSITIVE[0], SENSITIVE[2]] {
        let mut fields = ok(Decoder::default().block(&fixture_block(1), 0, 1));
        let positions: Vec<_> = fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.name == name)
            .map(|(i, _)| i)
            .collect();
        fields.swap(positions[0], positions[1]);
        rejects(binding(&fields, 1, b"127.0.0.1:1"), Kind::Binding);
    }
    // Cross-name regular order is NOT a requirement; per-name duplicate order is.
    let mut fields = ok(Decoder::default().block(&fixture_block(1), 0, 1));
    let begin = fields
        .iter()
        .position(|f| !f.name.starts_with(b":"))
        .unwrap();
    fields[begin..].sort_by(|a, b| b.name.cmp(&a.name));
    ok(binding(&fields, 1, b"127.0.0.1:1"));
    fields.swap(0, begin);
    rejects(binding(&fields, 1, b"127.0.0.1:1"), Kind::Binding);
}

#[test]
fn oracle_rejects_malformed_frames_huffman_and_indices() {
    // An accept error must have the SAME permitted I/O kind observed by the
    // tee, and only after all successful sessions and all upload EOFs.
    for (terminal, kind) in [
        (Terminal::Reset, io::ErrorKind::ConnectionReset),
        (Terminal::Aborted, io::ErrorKind::ConnectionAborted),
    ] {
        let mut c = fixture_capture();
        c.terminal = terminal;
        assert!(
            terminal_error(Some(kind), &c, true, 3),
            "classified terminal reset/abort"
        );
        assert!(
            !terminal_error(Some(kind), &c, false, 3),
            "reset before successful sessions"
        );
        assert!(
            !terminal_error(Some(kind), &c, true, 2),
            "reset before upload EOFs"
        );
        assert!(
            !terminal_error(Some(io::ErrorKind::BrokenPipe), &c, true, 3),
            "other I/O is not accepted"
        );
        assert!(
            !terminal_error(None, &c, true, 3),
            "protocol error is not accepted"
        );
        c.overflow = true;
        assert!(
            !terminal_error(Some(kind), &c, true, 3),
            "reset cannot hide overflow"
        );
    }
    // Eight padding bits; zero padding after '0'; embedded EOS.
    for bytes in [vec![0xff], vec![0x00], vec![0xff, 0xff, 0xff, 0xff]] {
        rejects(unhuff(&bytes, 0), Kind::Huffman);
    }
    assert!(
        ok(unhuff(&[0x07], 0)) == b"0",
        "valid short EOS-prefix padding"
    );
    for (bytes, kind) in [
        (vec![0x00, 1, b'x', 5, b'a'], Kind::Framing),
        (vec![0xff], Kind::Framing),
        (vec![0xff, 0x80], Kind::Framing),
        (vec![0xff, 0x80, 0x80, 0x80, 0x80, 0], Kind::Bounds),
        (vec![0xff, 0xff, 0xff, 0x7f], Kind::Bounds),
        (vec![0x20], Kind::Unsupported),
        (vec![0x80], Kind::Index),
        (vec![0xbe], Kind::Index),
        (vec![0x00, 0xff, 0x82, 0x80, 0x04], Kind::Bounds), // string length >65536
        (vec![0x00, 1, b'x', 0x81, 0xff], Kind::Huffman),
    ] {
        rejects(Decoder::default().block(&bytes, 0, 0), kind);
    }
    let mut oversized = Vec::new();
    literal(&mut oversized, 0x40, 0, b"x-public", &vec![b'a'; 4096]);
    rejects(Decoder::default().block(&oversized, 0, 0), Kind::Bounds); // Would require eviction/clear.
    let mut entries = Vec::new();
    for _ in 0..33 {
        literal(&mut entries, 0x40, 0, b"x", b"a");
    }
    rejects(Decoder::default().block(&entries, 0, 0), Kind::Bounds);
    let fields = vec![0x82u8; 65];
    rejects(Decoder::default().block(&fields, 0, 0), Kind::Bounds);
    let base = fixture_capture();
    let authority = b"127.0.0.1:1";
    let trace = ok(parse(&base, authority));
    for end in 0..base.bytes.len() {
        rejects(
            parse(
                &Capture {
                    bytes: base.bytes[..end].to_vec(),
                    overflow: false,
                    terminal: Terminal::Eof,
                },
                authority,
            ),
            Kind::Framing,
        );
    }
    let first = trace.blocks[0].frame;
    let mut bad_preface = fixture_capture();
    bad_preface.bytes[0] ^= 1;
    rejects(parse(&bad_preface, authority), Kind::Framing);
    // Explicit incomplete-block arrangements: EOF, wrong-stream continuation,
    // and interleaved DATA. S1 stops at missing END_HEADERS in all three.
    for next in [None, Some((9, 3)), Some((0, 1))] {
        let mut c = Capture {
            bytes: PREFACE.to_vec(),
            overflow: false,
            terminal: Terminal::Eof,
        };
        frame(&mut c.bytes, 4, 0, 0, &[]);
        frame(&mut c.bytes, 1, 0, 1, &fixture_block(1));
        if let Some((kind, stream)) = next {
            frame(&mut c.bytes, kind, 1, stream, &[]);
        }
        rejects(parse(&c, authority), Kind::Framing);
    }
    let first_data = first + 9 + fixture_block(1).len();
    for flags in [0, 2, 3] {
        let mut c = fixture_capture();
        c.bytes[first_data + 4] = flags;
        rejects(parse(&c, authority), Kind::Framing);
    }
    for flags in [0, 1, 5, 0x0c, 0x24] {
        let mut c = fixture_capture();
        c.bytes[first + 4] = flags;
        rejects(parse(&c, authority), Kind::Framing);
    }
    // CONTINUATION with no HEADERS, wrong stream, and interleaved DATA while
    // END_HEADERS is missing all fail closed in S1, never reassembled.
    for kind in [9, 0, 2, 5, 6, 255] {
        let mut c = fixture_capture();
        c.bytes[first + 3] = kind;
        rejects(parse(&c, authority), Kind::Framing);
    }
    for stream in [3u32, 7, 0x80000001] {
        let mut c = fixture_capture();
        c.bytes[first + 5..first + 9].copy_from_slice(&stream.to_be_bytes());
        rejects(parse(&c, authority), Kind::Framing);
    }
    let mut replay = fixture_capture();
    let second = trace.blocks[1].frame;
    replay.bytes[second + 5..second + 9].copy_from_slice(&1u32.to_be_bytes());
    rejects(parse(&replay, authority), Kind::Framing);
    let mut c = fixture_capture();
    c.bytes[first] = 0;
    c.bytes[first + 1] = 0x40;
    c.bytes[first + 2] = 1;
    rejects(parse(&c, authority), Kind::Bounds);
    let mut c = fixture_capture();
    c.overflow = true;
    rejects(parse(&c, authority), Kind::Bounds);
    let c = Capture {
        bytes: vec![0; CAP + 1],
        overflow: false,
        terminal: Terminal::Eof,
    };
    rejects(parse(&c, authority), Kind::Bounds);
    for terminal in [Terminal::Missing, Terminal::Other] {
        let mut c = fixture_capture();
        c.terminal = terminal;
        rejects(parse(&c, authority), Kind::Framing);
    }
    for terminal in [Terminal::Reset, Terminal::Aborted] {
        let mut c = fixture_capture();
        c.terminal = terminal;
        ok(parse(&c, authority));
        c.bytes.pop();
        rejects(parse(&c, authority), Kind::Framing);
    }
    let mut c = fixture_capture();
    frame(&mut c.bytes, 3, 0, 1, &[0; 4]);
    rejects(parse(&c, authority), Kind::Premise);
    let mut c = fixture_capture();
    frame(&mut c.bytes, 7, 0, 0, &[0; 8]);
    ok(parse(&c, authority));
    c.bytes.push(0);
    rejects(parse(&c, authority), Kind::Framing);
    let mut c = fixture_capture();
    frame(&mut c.bytes, 7, 0, 0, &[0, 0, 0, 0, 0, 0, 0, 1]);
    rejects(parse(&c, authority), Kind::Framing);
    let mut c = Capture {
        bytes: PREFACE.to_vec(),
        overflow: false,
        terminal: Terminal::Eof,
    };
    frame(&mut c.bytes, 4, 0, 0, &[]);
    frame(&mut c.bytes, 7, 0, 0, &[0; 8]);
    rejects(parse(&c, authority), Kind::Framing);
    let mut c = fixture_capture();
    frame(&mut c.bytes, 4, 0, 0, &[]);
    rejects(parse(&c, authority), Kind::Framing);
    let mut c = fixture_capture();
    frame(&mut c.bytes, 4, 1, 0, &[]);
    frame(&mut c.bytes, 4, 1, 0, &[]);
    rejects(parse(&c, authority), Kind::Framing);
    let mut c = fixture_capture();
    frame(&mut c.bytes, 8, 0, 0, &[0; 4]);
    rejects(parse(&c, authority), Kind::Framing);
    let mut c = fixture_capture();
    for _ in 0..9 {
        frame(&mut c.bytes, 8, 0, 0, &[0, 0, 0, 1]);
    }
    rejects(parse(&c, authority), Kind::Framing);
    let mut c = fixture_capture();
    frame(&mut c.bytes, 0, 1, 5, &[]);
    rejects(parse(&c, authority), Kind::Framing);
}
