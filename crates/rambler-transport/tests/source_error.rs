use rambler_transport::SourceError;
use std::error::Error;
use std::io;

#[test]
fn original_error_can_be_downcast_without_extending_its_borrow() {
    let wrapped = SourceError::new(io::Error::new(
        io::ErrorKind::InvalidData,
        "private-source-detail",
    ));
    let cloned = wrapped.clone();
    let original = cloned
        .original()
        .downcast_ref::<io::Error>()
        .expect("explicit accessor preserves the concrete error type");
    assert_eq!(original.kind(), io::ErrorKind::InvalidData);
    assert_eq!(original.to_string(), "private-source-detail");
    assert!(wrapped.source().is_none());
    assert!(!format!("{wrapped:?} {wrapped}").contains("private-source-detail"));
}
