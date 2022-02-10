use std::{error::Error, path::PathBuf};

use clap::Parser;

mod legacy {
    use serde::Deserialize;
    use serde_yaml::Value;

    #[derive(Debug, Deserialize, Clone)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub struct Test {
        pub description: String,
        pub min_server_version: String,
        pub max_server_version: Option<String>,
        pub fail_point: Option<serde_yaml::Mapping>,
        pub target: Target,
        pub topology: Vec<String>,
        pub change_stream_pipeline: Vec<Value>,
        pub change_stream_options: Option<Value>,
        pub operations: Vec<Operation>,
        pub expectations: Option<Vec<serde_yaml::Mapping>>,
        pub result: TestResult,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct File {
        pub database_name: String,
        pub collection_name: String,
        pub database2_name: Option<String>,
        pub collection2_name: Option<String>,
        pub tests: Vec<Test>,
    }

    #[derive(Debug, Deserialize, Clone)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub enum Target {
        Collection,
        Database,
        Client,
    }

    #[derive(Debug, Deserialize, Clone)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub struct Operation {
        pub name: String,
        pub database: String,
        pub collection: String,
        pub arguments: Option<serde_yaml::Mapping>,
    }

    #[derive(Debug, Deserialize, Clone)]
    #[serde(untagged, rename_all = "camelCase", deny_unknown_fields)]
    pub enum TestResult {
        Error { error: ExpectError },
        Success { success: Vec<serde_yaml::Mapping> },
    }

    #[derive(Debug, Deserialize, Clone)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub struct ExpectError {
        pub code: i32,
    }
}

mod unified {
    use serde::Serialize;
    use serde_yaml::Value;

    use super::legacy;

    #[serde_with::skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Test {
        description: String,
        run_on_requirements: Vec<RunOnRequirements>,
        operations: Vec<Operation>,
        expect_events: Option<Vec<ExpectEvents>>,
    }

    #[serde_with::skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct RunOnRequirements {
        min_server_version: String,
        max_server_version: Option<String>,
        topologies: Vec<String>,
    }

    #[serde_with::skip_serializing_none]
    #[derive(Debug, Default, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Operation {
        name: String,
        object: String,
        arguments: Option<serde_yaml::Mapping>,
        save_result_as_entity: Option<String>,
        expect_result: Option<serde_yaml::Mapping>,
        expect_error: Option<ExpectError>,
    }

    #[derive(Debug, Default, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ExpectEvents {
        client: String,
        event_match: String,
        events: Vec<serde_yaml::Mapping>,
    }

    #[derive(Debug, Default, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ExpectError {
        error_code: i32,
    }

    const CLIENT_NAME: &str = "client0";
    const GLOBAL_CLIENT_NAME: &str = "globalClient";
    const DB1_NAME: &str = "database0";
    const DB2_NAME: &str = "database1";
    const COLL1_NAME: &str = "collection0";
    const COLL2_NAME: &str = "collection1";

    impl Test {
        pub fn parse(args: &super::Args, file: &legacy::File, old: legacy::Test) -> Self {
            Self {
                description: old.description,
                run_on_requirements: vec![RunOnRequirements {
                    min_server_version: old.min_server_version,
                    max_server_version: old.max_server_version,
                    topologies: old.topology,
                }],
                operations: {
                    let mut out = vec![];
                    // Fail point
                    if let Some(fp) = old.fail_point {
                        out.push(Operation {
                            name: "failPoint".to_string(),
                            object: "testRunner".to_string(),
                            arguments: Some(
                                vec![
                                    (ys("client"), ys(GLOBAL_CLIENT_NAME)),
                                    (ys("failPoint"), Value::Mapping(fp)),
                                ]
                                .into_iter()
                                .collect(),
                            ),
                            ..Operation::default()
                        });
                    }
                    // Initial createChangeStream
                    out.push(Operation {
                        name: "createChangeStream".to_string(),
                        object: match old.target {
                            legacy::Target::Collection => COLL1_NAME,
                            legacy::Target::Database => DB1_NAME,
                            legacy::Target::Client => CLIENT_NAME,
                        }
                        .to_string(),
                        arguments: Some(
                            {
                                let mut out = vec![(
                                    ys("pipeline"),
                                    Value::Sequence(old.change_stream_pipeline),
                                )];
                                if let Some(options) = old.change_stream_options {
                                    out.push((ys("options"), options));
                                }
                                out
                            }
                            .into_iter()
                            .collect(),
                        ),
                        save_result_as_entity: Some("changeStream0".to_string()),
                        ..Operation::default()
                    });
                    // Test operations
                    out.extend(
                        old.operations
                            .into_iter()
                            .map(|op| parse_operation(file, op)),
                    );
                    // Test results
                    match old.result {
                        legacy::TestResult::Success { success } => {
                            out.extend(success.into_iter().map(|suc| parse_success(file, suc)))
                        }
                        legacy::TestResult::Error { error } => {
                            out.push(Operation {
                                name: "iterateUntilDocumentOrError".to_string(),
                                object: "changeStream0".to_string(),
                                expect_error: Some(ExpectError {
                                    error_code: error.code,
                                }),
                                ..Operation::default()
                            });
                        }
                    }
                    out
                },
                expect_events: old.expectations.map(|mut exps| {
                    let len = exps.len();
                    for (ix, exp) in (&mut exps).into_iter().enumerate() {
                        if let Some(Value::Mapping(mut cse)) =
                            exp.remove(&ys("command_started_event"))
                        {
                            if let Some(val) = cse.remove(&ys("command_name")) {
                                cse.insert(ys("commandName"), val);
                            }
                            if let Some(mut val) = cse.remove(&ys("database_name")) {
                                match val {
                                    Value::String(s) if s == file.database_name => {
                                        val = Value::String(DB1_NAME.to_string());
                                    }
                                    _ => (),
                                }
                                cse.insert(ys("databaseName"), val);
                            }
                            let mut val = Value::Mapping(cse);
                            // The *-resume-*.yml test files contain event expecations that are
                            // missing the `resumeAfter` clause, so this
                            // adds a matcher that will work whether or
                            // not that clause is present.
                            if args.fix_resume_event && ix == len - 1 {
                                fn singleton(key: &str, value: Value) -> Value {
                                    Value::Mapping(vec![(ys(key), value)].into_iter().collect())
                                }
                                val["command"]["pipeline"][0]["$changeStream"] = singleton(
                                    "resumeAfter",
                                    singleton(
                                        "$$unsetOrMatches",
                                        singleton("$$exists", Value::Bool(true)),
                                    ),
                                );
                            }
                            exp.insert(ys("commandStartedEvent"), val);
                        }
                        fix_names(file, exp);
                        fix_placeholders(exp);
                    }
                    vec![ExpectEvents {
                        client: CLIENT_NAME.to_string(),
                        event_match: "prefix".to_string(),
                        events: exps,
                    }]
                }),
            }
        }
    }

    fn ys<S: Into<String>>(s: S) -> Value {
        Value::String(s.into())
    }

    const GLOBAL_DB1_NAME: &str = "globalDatabase0";
    const GLOBAL_DB2_NAME: &str = "globalDatabase1";
    const GLOBAL_COLL1_NAME: &str = "globalCollection0";
    const GLOBAL_COLL2_NAME: &str = "globalCollection1";
    const GLOBAL_DB2_COLL1_NAME: &str = "globalDb1Collection0";
    const GLOBAL_DB1_COLL2_NAME: &str = "globalDb0Collection1";

    fn parse_operation(file: &legacy::File, old: legacy::Operation) -> Operation {
        let object = {
            let coll_num = if old.collection == file.collection_name {
                1
            } else if Some(&old.collection) == file.collection2_name.as_ref() {
                2
            } else {
                panic!("unexpected collection name {:?}", old.collection);
            };
            let db_num = if old.database == file.database_name {
                1
            } else if Some(&old.database) == file.database2_name.as_ref() {
                2
            } else {
                panic!("unexpected database name {:?}", old.database);
            };
            match (db_num, coll_num) {
                (1, 1) => GLOBAL_COLL1_NAME,
                (1, 2) => GLOBAL_DB1_COLL2_NAME,
                (2, 1) => GLOBAL_DB2_COLL1_NAME,
                (2, 2) => GLOBAL_DB2_NAME,
                _ => panic!("invalid target {:?}", (db_num, coll_num)),
            }
        }
        .to_string();
        let mut arguments = old.arguments;
        if let Some(args) = &mut arguments {
            fix_names(file, args);
        }
        Operation {
            name: old.name,
            object,
            arguments,
            ..Operation::default()
        }
    }

    fn fix_names(file: &legacy::File, map: &mut serde_yaml::Mapping) {
        visit_mut(map, &|_, val| match val {
            Value::String(s) => {
                if s == &file.database_name {
                    *s = DB1_NAME.to_string()
                } else if Some(&*s) == file.database2_name.as_ref() {
                    *s = DB2_NAME.to_string()
                } else if s == &file.collection_name {
                    *s = COLL1_NAME.to_string()
                } else if Some(&*s) == file.collection2_name.as_ref() {
                    *s = COLL2_NAME.to_string()
                }
            }
            _ => (),
        });
    }

    fn fix_placeholders(map: &mut serde_yaml::Mapping) {
        visit_mut(map, &|_, val| match val {
            Value::Number(num) if num.as_i64() == Some(42) => *val = exists(),
            Value::String(s) if s == "42" => *val = exists(),
            _ => (),
        });
    }

    fn parse_success(file: &legacy::File, mut suc: serde_yaml::Mapping) -> Operation {
        visit_mut(&mut suc, &|key, val| {
            if key == "fullDocument" {
                if let Value::Mapping(map) = val {
                    if !map.contains_key(&ys("_id")) {
                        map.insert(ys("_id"), exists());
                    }
                }
            }
        });
        fix_names(file, &mut suc);
        fix_placeholders(&mut suc);
        Operation {
            name: "iterateUntilDocumentOrError".to_string(),
            object: "changeStream0".to_string(),
            expect_result: Some(suc),
            ..Operation::default()
        }
    }

    fn exists() -> Value {
        Value::Mapping([(ys("$$exists"), Value::Bool(true))].into_iter().collect())
    }

    fn visit_mut<F>(root: &mut serde_yaml::Mapping, visitor: &F)
    where
        F: Fn(&str, &mut Value),
    {
        for (key, value) in root.iter_mut() {
            visitor(key.as_str().unwrap(), value);
            if let Value::Mapping(map) = value {
                visit_mut(map, visitor);
            }
        }
    }

    pub fn postprocess(text: &mut String) {
        *text = text
            .replace(
                "saveResultAsEntity: changeStream0",
                "saveResultAsEntity: &changeStream0 changeStream0",
            )
            .replace_with_ref("changeStream0")
            .replace_with_ref(CLIENT_NAME)
            .replace_with_ref(DB1_NAME)
            .replace_with_ref(DB2_NAME)
            .replace_with_ref(COLL1_NAME)
            .replace_with_ref(COLL2_NAME)
            .replace_with_ref(GLOBAL_DB1_NAME)
            .replace_with_ref(GLOBAL_DB2_NAME)
            .replace_with_ref(GLOBAL_COLL1_NAME)
            .replace_with_ref(GLOBAL_COLL2_NAME)
            .replace_with_ref(GLOBAL_DB1_COLL2_NAME)
            .replace_with_ref(GLOBAL_DB2_COLL1_NAME);
    }

    trait StringExt {
        fn replace_with_ref(&self, name: &str) -> String;
    }

    impl StringExt for String {
        fn replace_with_ref(&self, name: &str) -> String {
            self.replace(&format!(": {}", name), &format!(": *{}", name))
        }
    }
}

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(short, long)]
    input: PathBuf,
    #[clap(short, long)]
    test: usize,
    #[clap(short, long)]
    fix_resume_event: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let input = std::fs::read_to_string(&args.input)?;
    let file: legacy::File = serde_yaml::from_str(&input)?;
    println!("Parsing test {} of {}", args.test + 1, file.tests.len());
    let test = &file.tests[args.test];

    let out = unified::Test::parse(&args, &file, test.clone());
    let mut text = serde_yaml::to_string(&out)?;
    unified::postprocess(&mut text);
    println!("{}", text);

    Ok(())
}
