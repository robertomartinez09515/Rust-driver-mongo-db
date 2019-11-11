#[cfg(test)]
mod test;

use std::collections::HashMap;

use bson::{bson, doc, Document};

use crate::{
    bson_util,
    cmap::{Command, CommandResponse, StreamDescription},
    error::{ErrorKind, Result},
    operation::{append_options, Operation, WriteResponseBody},
    options::InsertManyOptions,
    results::InsertManyResult,
    Namespace,
};

#[derive(Debug)]
pub(crate) struct Insert {
    ns: Namespace,
    documents: Vec<Document>,
    options: Option<InsertManyOptions>,
}

impl Insert {
    pub(crate) fn new(
        ns: Namespace,
        documents: Vec<Document>,
        options: Option<InsertManyOptions>,
    ) -> Self {
        Self {
            ns,
            options,
            documents: documents
                .into_iter()
                .map(|mut d| {
                    bson_util::add_id(&mut d);
                    d
                })
                .collect(),
        }
    }
}

impl Operation for Insert {
    type O = InsertManyResult;
    const NAME: &'static str = "insert";

    fn build(&self, description: &StreamDescription) -> Result<Command> {
        let mut body = doc! {
            Self::NAME: self.ns.coll.clone(),
            "documents": bson_util::to_bson_array(&self.documents),
        };
        append_options(&mut body, self.options.as_ref())?;

        Ok(Command::new(
            Self::NAME.to_string(),
            self.ns.db.clone(),
            body,
        ))
    }

    fn handle_response(&self, response: CommandResponse) -> Result<Self::O> {
        let body: WriteResponseBody = response.body()?;
        body.validate()?;

        let mut map = HashMap::new();
        for (i, doc) in self.documents.iter().enumerate() {
            map.insert(
                i,
                doc.get("_id")
                    .ok_or_else(|| ErrorKind::OperationError {
                        message: "missing _id in inserted document".to_string(),
                    })?
                    .clone(),
            );
        }
        Ok(InsertManyResult { inserted_ids: map })
    }
}
