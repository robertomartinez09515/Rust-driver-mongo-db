use bson::Timestamp;

use super::RunCommand;
use crate::{
    bson::doc,
    cmap::StreamDescription,
    operation::{test::handle_response_test, Operation},
};

#[test]
fn build() {
    let mut op = RunCommand::new("foo".into(), doc! { "hello": 1 }, None, None).unwrap();
    assert!(op.selection_criteria().is_none());

    let command = op.build(&StreamDescription::new_testing()).unwrap();

    assert_eq!(command.name, "hello");
    assert_eq!(command.target_db, "foo");
    assert_eq!(
        command
            .body
            .get("hello")
            .and_then(crate::bson_util::get_int),
        Some(1)
    );
}

#[test]
fn handle_success() {
    let op = RunCommand::new("foo".into(), doc! { "hello": 1 }, None, None).unwrap();

    let doc = doc! {
        "ok": 1,
        "some": "field",
        "other": true,
        "$clusterTime": {
            "clusterTime": Timestamp {
                time: 123,
                increment: 345,
            },
            "signature": {}
        }
    };
    let result_doc = handle_response_test(&op, doc.clone()).unwrap();
    assert_eq!(result_doc, doc);
}
