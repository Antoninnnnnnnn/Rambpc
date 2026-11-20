use super::{CONSENT_FILE, ConsentRecord, Store, StoreError, write_atomic};
use crate::{
    ipc::{PersonalRules, PersonalRulesInput},
    settings::{HISTORY_DAYS_CHOICES, HistoryOptions},
    text::{Expression, LexiconEntry, RuleConflict, validate_rules},
};
use serde::{Deserialize, Serialize};
use std::{fs, io};
use zeroize::Zeroizing;

const RULES_FILE: &str = "personal-rules.bin";
const HISTORY_POLICY: &str = "history-policy.bin";

#[derive(Serialize, Deserialize)]
struct RulesRecord {
    version: u32,
    lexicon: Vec<LexiconEntry>,
    expressions: Vec<Expression>,
}

#[derive(Serialize, Deserialize)]
struct HistoryRecord {
    version: u32,
    options: HistoryOptions,
}

impl Store {
    /// A profile whose privacy choices are unreadable opens with ordinary
    /// preferences only. Do not decrypt the other files in that profile.
    pub(crate) fn withheld_personal_rules(&self) -> PersonalRules {
        PersonalRules {
            lexicon: Vec::new(),
            expressions: Vec::new(),
            unreadable: self
                .root()
                .dir
                .join(RULES_FILE)
                .try_exists()
                .unwrap_or(true),
        }
    }

    pub(crate) fn load_personal_rules(&self) -> PersonalRules {
        match self.read_protected::<RulesRecord>(RULES_FILE) {
            Ok(Some(record))
                if record.version == 1
                    && validate_rules(&record.lexicon, &record.expressions).is_empty() =>
            {
                PersonalRules {
                    lexicon: record.lexicon,
                    expressions: record.expressions,
                    unreadable: false,
                }
            }
            other => PersonalRules {
                lexicon: Vec::new(),
                expressions: Vec::new(),
                unreadable: !matches!(other, Ok(None)),
            },
        }
    }

    /// Nonempty conflicts mean nothing was changed on disk or in memory.
    pub(crate) fn save_personal_rules(
        &self,
        input: &PersonalRulesInput,
    ) -> Result<Vec<RuleConflict>, StoreError> {
        let conflicts = validate_rules(&input.lexicon, &input.expressions);
        if conflicts.is_empty() && !self.root().ephemeral {
            self.write_protected(
                RULES_FILE,
                &RulesRecord {
                    version: 1,
                    lexicon: input.lexicon.clone(),
                    expressions: input.expressions.clone(),
                },
            )?;
        }
        Ok(conflicts)
    }

    pub(crate) fn load_history_options(&self) -> Result<Option<HistoryOptions>, StoreError> {
        self.read_protected::<HistoryRecord>(HISTORY_POLICY)?
            .map(|record| {
                if record.version == 1
                    && HISTORY_DAYS_CHOICES.contains(&record.options.retention_days)
                {
                    Ok(record.options)
                } else {
                    Err(StoreError::Protect)
                }
            })
            .transpose()
    }

    pub(crate) fn save_history_options(&self, options: &HistoryOptions) -> Result<(), StoreError> {
        self.write_protected(
            HISTORY_POLICY,
            &HistoryRecord {
                version: 1,
                options: options.clone(),
            },
        )
    }

    /// Check the privacy choices before opening, purging or migrating the DB.
    pub(crate) fn protected_profile_unreadable(&self) -> bool {
        self.load_history_options().is_err()
            || !matches!(
                self.read_protected::<ConsentRecord>(CONSENT_FILE),
                Ok(None) | Ok(Some(ConsentRecord { version: 1, .. }))
            )
    }

    fn read_protected<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<Option<T>, StoreError> {
        let bytes = match fs::read(self.root().dir.join(name)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(StoreError::Io),
        };
        let plain = self
            .protector
            .unprotect(&bytes)
            .map_err(|_| StoreError::Protect)?;
        serde_json::from_slice(&plain)
            .map(Some)
            .map_err(|_| StoreError::Protect)
    }

    fn write_protected(&self, name: &str, record: &impl Serialize) -> Result<(), StoreError> {
        if self.root().ephemeral {
            return Err(StoreError::Ephemeral);
        }
        let plain = Zeroizing::new(serde_json::to_vec(record).map_err(|_| StoreError::Io)?);
        let sealed = self
            .protector
            .protect(&plain)
            .map_err(|_| StoreError::Protect)?;
        write_atomic(&self.root().dir.join(name), &sealed).map_err(|_| StoreError::Io)
    }
}
