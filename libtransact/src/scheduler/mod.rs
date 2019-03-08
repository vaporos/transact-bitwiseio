/*
 * Copyright 2019 Bitwise IO, Inc.
 * Copyright 2019 Cargill Incorporated
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 * -----------------------------------------------------------------------------
 */

//! Batch scheduling with transaction execution APIs
//!
//! A `Scheduler` is used to execute one or more `Batch` objects, receiving
//! a `BatchExecutionResult` as a result of the execution of a `Batch`.  A `Batch` contains one or
//! more `Transaction` objects; each `Transaction` within a `Batch` is executed and then
//! consolidated to generate a `BatchExecutionResult`.
//!
//! In order for a `Scheduler` to execute batches, its associated `SchedulerExecutionInterface`
//! must be consumed by a component responsible for iterating over the `Transaction`s and providing
//! `TransactionExecutionResult`s back to the `Scheduler` via the `SchedulerExecutionInterface`.

pub mod parallel;

use crate::context::ContextId;
use crate::protocol::batch::BatchPair;
use crate::protocol::receipt::TransactionReceipt;
use crate::protocol::transaction::TransactionPair;

/// A transation and associated information required to execute it.
pub struct ExecutionTask {
    pair: TransactionPair,
    context_id: ContextId,
}

impl ExecutionTask {
    /// Create a new `ExecutionPair`.
    pub fn new(pair: TransactionPair, context_id: ContextId) -> Self {
        ExecutionTask { pair, context_id }
    }

    /// The transaction to be executed.
    pub fn pair(&self) -> &TransactionPair {
        &self.pair
    }

    /// The identifier of the context to be used when accessing state.
    pub fn context_id(&self) -> &ContextId {
        &self.context_id
    }

    /// Decompose into its components.
    pub fn take(self) -> (TransactionPair, ContextId) {
        (self.pair, self.context_id)
    }
}

/// Result from executing an invalid transaction.
#[derive(Debug, PartialEq)]
pub struct InvalidTransactionResult {
    /// Transaction identifier.
    pub transaction_id: String,

    /// Human-readable reason explaining why the transaction was invalid.
    pub error_message: String,

    /// Transaction-specific error data which can be interpreted by clients
    /// familiar with this transaction's family.
    pub error_data: Vec<u8>,
}

/// Result from executing a transaction.
pub enum TransactionExecutionResult {
    /// The transation was invalid.
    Invalid(InvalidTransactionResult),

    /// The transation was valid and execution produced a TransactionReceipt.
    Valid(TransactionReceipt),
}

/// Result of executing a batch.
pub struct BatchExecutionResult {
    /// The `BatchPair` which was executed.
    pub batch: BatchPair,

    /// The results for each transaction in the batch.
    pub results: Vec<TransactionExecutionResult>,
}

#[derive(Debug, PartialEq)]
pub enum ExecutionTaskCompletionNotification {
    /// The transation was invalid.
    Invalid(ContextId, InvalidTransactionResult),

    /// The transation was valid.
    Valid(ContextId),
}

/// Schedules batches and transactions and returns execution results.
pub trait Scheduler {
    /// Sets a callback to receive results from processing batches. The order
    /// the results are received is not guarenteed to be the same order as the
    /// batches were added with `add_batch`. If callback is called with None,
    /// all batch results have been sent.
    fn set_result_callback(&mut self, callback: Box<Fn(Option<BatchExecutionResult>) + Send>);

    /// Adds a BatchPair to the scheduler.
    fn add_batch(&mut self, batch: BatchPair);

    /// Drops any unscheduled transactions from this scheduler. Any already
    /// scheduled transactions will continue to execute.
    ///
    /// Returns a `Vec` of the dropped `BatchPair`s.
    fn cancel(&mut self) -> Vec<BatchPair>;

    /// Finalizes the scheduler, which will disable the ability to add more
    /// batches. After this is called, `add_batch()` will be return a
    /// FinalizedSchedulerError.
    fn finalize(&mut self);

    /// Returns an iterator that returns transactions to be executed.
    fn take_task_iterator(&mut self) -> Box<dyn Iterator<Item = ExecutionTask> + Send>;

    /// Returns a newly allocated ExecutionTaskCompletionNotifier which allows
    /// sending a notification to the scheduler that indicates the task has
    /// been executed.
    fn new_notifier(&mut self) -> Box<dyn ExecutionTaskCompletionNotifier>;
}

/// Allows sending a notification to the scheduler that execution of a task
/// has completed.
pub trait ExecutionTaskCompletionNotifier: Send {
    /// Sends a notification to the scheduler.
    fn notify(&self, notification: ExecutionTaskCompletionNotification);
}
