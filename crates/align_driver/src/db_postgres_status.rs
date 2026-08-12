//! One exhaustive libpq 17 result-status authority for compiler-side database tools.

use std::ffi::c_int;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PostgresResultStatus {
    EmptyQuery,
    CommandOk,
    TuplesOk,
    CopyOut,
    CopyIn,
    BadResponse,
    NonfatalError,
    FatalError,
    CopyBoth,
    SingleTuple,
    PipelineSync,
    PipelineAborted,
    TuplesChunk,
    Unknown(c_int),
}

pub(crate) fn classify(status: c_int) -> PostgresResultStatus {
    match status {
        0 => PostgresResultStatus::EmptyQuery,
        1 => PostgresResultStatus::CommandOk,
        2 => PostgresResultStatus::TuplesOk,
        3 => PostgresResultStatus::CopyOut,
        4 => PostgresResultStatus::CopyIn,
        5 => PostgresResultStatus::BadResponse,
        6 => PostgresResultStatus::NonfatalError,
        7 => PostgresResultStatus::FatalError,
        8 => PostgresResultStatus::CopyBoth,
        9 => PostgresResultStatus::SingleTuple,
        10 => PostgresResultStatus::PipelineSync,
        11 => PostgresResultStatus::PipelineAborted,
        12 => PostgresResultStatus::TuplesChunk,
        value => PostgresResultStatus::Unknown(value),
    }
}

pub(crate) fn tool_must_close(status: PostgresResultStatus) -> bool {
    matches!(
        status,
        PostgresResultStatus::CopyOut
            | PostgresResultStatus::CopyIn
            | PostgresResultStatus::CopyBoth
            | PostgresResultStatus::SingleTuple
            | PostgresResultStatus::PipelineSync
            | PostgresResultStatus::PipelineAborted
            | PostgresResultStatus::TuplesChunk
            | PostgresResultStatus::Unknown(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_libpq_17_status_table_and_unknown_sentinels_are_fail_closed() {
        let expected = [
            PostgresResultStatus::EmptyQuery,
            PostgresResultStatus::CommandOk,
            PostgresResultStatus::TuplesOk,
            PostgresResultStatus::CopyOut,
            PostgresResultStatus::CopyIn,
            PostgresResultStatus::BadResponse,
            PostgresResultStatus::NonfatalError,
            PostgresResultStatus::FatalError,
            PostgresResultStatus::CopyBoth,
            PostgresResultStatus::SingleTuple,
            PostgresResultStatus::PipelineSync,
            PostgresResultStatus::PipelineAborted,
            PostgresResultStatus::TuplesChunk,
        ];
        for (status, expected) in expected.into_iter().enumerate() {
            assert_eq!(classify(status as c_int), expected);
        }
        for status in [-1, 13, c_int::MAX] {
            assert_eq!(classify(status), PostgresResultStatus::Unknown(status));
            assert!(tool_must_close(classify(status)));
        }
        for status in [3, 4, 8, 9, 10, 11, 12] {
            assert!(tool_must_close(classify(status)), "status {status}");
        }
        for status in [0, 1, 2, 5, 6, 7] {
            assert!(!tool_must_close(classify(status)), "status {status}");
        }
    }

    #[test]
    fn postgres_tool_results_fail_closed_before_followup_native_work() {
        crate::db_migrate_native::postgres_status_test_support::assert_migration_consumers();
        crate::db_prepare_native::postgres_status_test_support::assert_prepare_consumers();
    }
}
