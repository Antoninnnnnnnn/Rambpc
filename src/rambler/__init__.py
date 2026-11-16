"""Direct, reusable Rambler client; optional native encoding is imported separately."""

from .credentials import EnvironmentCredentials, MetadataProvider
from .engine import (
    ContextProvider,
    EngineEvent,
    IncompleteSessionError,
    RamblerEngine,
    S3SessionError,
    SessionMetrics,
)
from .protocol import EditingContext, ProtocolConfig, S3Response, S3Status, SpeechEvent
from .results import (
    AmbiguousResultError,
    ContextConflictError,
    DictationResult,
    ReplacementProposal,
    ResultLimitError,
    UnsupportedEditError,
    collect_dictation,
    replacement_proposal,
)
from .transport import DEFAULT_METHOD, DEFAULT_TARGET, GrpcTransport

__all__ = [
    "DEFAULT_METHOD", "DEFAULT_TARGET", "ContextProvider", "EditingContext", "EngineEvent",
    "EnvironmentCredentials", "GrpcTransport", "IncompleteSessionError",
    "MetadataProvider", "ProtocolConfig", "RamblerEngine", "S3Response",
    "S3SessionError", "S3Status", "SessionMetrics", "SpeechEvent",
    "AmbiguousResultError", "ContextConflictError", "DictationResult",
    "ReplacementProposal", "ResultLimitError", "UnsupportedEditError",
    "collect_dictation", "replacement_proposal",
]
