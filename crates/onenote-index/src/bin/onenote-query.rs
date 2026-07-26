use onenote_core::{OneNoteLoader, SourceId};
use onenote_index::protocol::{
    Request, RequestEnvelope, Response, ResponseEnvelope, PROTOCOL_VERSION,
};
use onenote_index::{Error as IndexError, SearchIndex};
use std::collections::{hash_map::Entry, HashMap};
use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

const RESULT_BATCH_SIZE: usize = 50;
type CancellationMap = Mutex<HashMap<String, Arc<AtomicBool>>>;

fn main() -> ExitCode {
    match database_argument() {
        Ok(database) => run(database),
        Err(message) => {
            let _ignored = writeln!(io::stderr(), "{message}");
            ExitCode::from(2)
        }
    }
}

fn run(database: PathBuf) -> ExitCode {
    let (response_sender, response_receiver) = mpsc::channel::<ResponseEnvelope>();
    let writer = thread::spawn(move || write_responses(response_receiver));
    let cancellations = Arc::new(CancellationMap::default());
    let (work_sender, work_receiver) = mpsc::channel();
    let worker_responses = response_sender.clone();
    let worker_cancellations = Arc::clone(&cancellations);
    let worker = thread::spawn(move || {
        worker_loop(
            database,
            work_receiver,
            &worker_responses,
            &worker_cancellations,
        );
    });

    let mut negotiated = false;
    read_requests(
        &work_sender,
        &response_sender,
        &cancellations,
        &mut negotiated,
    );
    drop(work_sender);
    let _worker_result = worker.join();
    drop(response_sender);
    if writer.join().is_ok_and(|result| result.is_ok()) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn read_requests(
    work: &mpsc::Sender<Work>,
    responses: &mpsc::Sender<ResponseEnvelope>,
    cancellations: &CancellationMap,
    negotiated: &mut bool,
) {
    for line in io::stdin().lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                send(
                    responses,
                    ResponseEnvelope::error("", "input_error", error.to_string()),
                );
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let envelope = match serde_json::from_str(&line) {
            Ok(envelope) => envelope,
            Err(error) => {
                send(
                    responses,
                    ResponseEnvelope::error("", "invalid_json", error.to_string()),
                );
                continue;
            }
        };
        if !dispatch(envelope, work, responses, cancellations, negotiated) {
            break;
        }
    }
}

fn dispatch(
    envelope: RequestEnvelope,
    work: &mpsc::Sender<Work>,
    responses: &mpsc::Sender<ResponseEnvelope>,
    cancellations: &CancellationMap,
    negotiated: &mut bool,
) -> bool {
    if let Request::Hello { supported_versions } = &envelope.request {
        negotiate(
            envelope.request_id,
            envelope.protocol_version,
            supported_versions,
            responses,
            negotiated,
        );
        return true;
    }
    if !*negotiated || envelope.protocol_version != PROTOCOL_VERSION {
        send(
            responses,
            ResponseEnvelope::error(
                envelope.request_id,
                "protocol_required",
                "successful hello negotiation is required",
            ),
        );
        return true;
    }

    match envelope.request {
        Request::Cancel { target_request_id } => {
            let accepted = lock_cancellations(cancellations)
                .get(&target_request_id)
                .is_some_and(|flag| {
                    flag.store(true, Ordering::Relaxed);
                    true
                });
            send(
                responses,
                ResponseEnvelope::new(
                    envelope.request_id,
                    Response::Cancelled {
                        target_request_id,
                        accepted,
                    },
                ),
            );
        }
        Request::Shutdown => {
            let _sent = work.send(Work::Shutdown {
                request_id: envelope.request_id,
            });
            return false;
        }
        request => {
            if !queue_operation(envelope.request_id, request, work, responses, cancellations) {
                return false;
            }
        }
    }
    true
}

fn negotiate(
    request_id: String,
    envelope_version: u32,
    supported_versions: &[u32],
    responses: &mpsc::Sender<ResponseEnvelope>,
    negotiated: &mut bool,
) {
    let response = if *negotiated {
        ResponseEnvelope::error(
            request_id,
            "already_negotiated",
            "protocol negotiation has already completed",
        )
    } else if envelope_version != PROTOCOL_VERSION
        || !supported_versions.contains(&PROTOCOL_VERSION)
    {
        ResponseEnvelope::error(
            request_id,
            "unsupported_protocol",
            "no mutually supported protocol version",
        )
    } else {
        *negotiated = true;
        ResponseEnvelope::new(
            request_id,
            Response::Hello {
                selected_version: PROTOCOL_VERSION,
            },
        )
    };
    send(responses, response);
}

fn queue_operation(
    request_id: String,
    request: Request,
    work: &mpsc::Sender<Work>,
    responses: &mpsc::Sender<ResponseEnvelope>,
    cancellations: &CancellationMap,
) -> bool {
    let cancellation = Arc::new(AtomicBool::new(false));
    match lock_cancellations(cancellations).entry(request_id.clone()) {
        Entry::Occupied(_) => {
            send(
                responses,
                ResponseEnvelope::error(
                    request_id,
                    "duplicate_request_id",
                    "request ID is already queued or active",
                ),
            );
            return true;
        }
        Entry::Vacant(entry) => {
            entry.insert(Arc::clone(&cancellation));
        }
    }
    if work
        .send(Work::Operation {
            request_id: request_id.clone(),
            request,
            cancellation,
        })
        .is_err()
    {
        lock_cancellations(cancellations).remove(&request_id);
        return false;
    }
    true
}

enum Work {
    Operation {
        request_id: String,
        request: Request,
        cancellation: Arc<AtomicBool>,
    },
    Shutdown {
        request_id: String,
    },
}

fn worker_loop(
    database: PathBuf,
    receiver: mpsc::Receiver<Work>,
    responses: &mpsc::Sender<ResponseEnvelope>,
    cancellations: &CancellationMap,
) {
    let mut index = match SearchIndex::open(database) {
        Ok(index) => index,
        Err(error) => {
            send(
                responses,
                ResponseEnvelope::error("", index_error_code(&error), error.to_string()),
            );
            return;
        }
    };
    for work in receiver {
        match work {
            Work::Operation {
                request_id,
                request,
                cancellation,
            } => {
                execute(&mut index, responses, &request_id, request, &cancellation);
                lock_cancellations(cancellations).remove(&request_id);
            }
            Work::Shutdown { request_id } => {
                send(
                    responses,
                    ResponseEnvelope::new(request_id, Response::Goodbye),
                );
                break;
            }
        }
    }
}

fn execute(
    index: &mut SearchIndex,
    responses: &mpsc::Sender<ResponseEnvelope>,
    request_id: &str,
    request: Request,
    cancellation: &AtomicBool,
) {
    let result: Result<(), ProtocolFailure> = match request {
        Request::IndexSource { path } => {
            index_source(index, responses, request_id, path, cancellation)
        }
        Request::RemoveSource { source_id } => index
            .remove_source(&source_id)
            .map(|removed| {
                send(
                    responses,
                    ResponseEnvelope::new(request_id, Response::Removed { removed }),
                );
            })
            .map_err(ProtocolFailure::from),
        Request::Search { query } => index
            .search(&query, cancellation)
            .map(|hits| send_result_batches(responses, request_id, hits))
            .map_err(ProtocolFailure::from),
        Request::ListSources => index
            .sources()
            .map(|sources| {
                send(
                    responses,
                    ResponseEnvelope::new(request_id, Response::Sources { sources }),
                );
            })
            .map_err(ProtocolFailure::from),
        Request::Verify => index
            .verify_integrity()
            .map(|()| {
                send(
                    responses,
                    ResponseEnvelope::new(request_id, Response::Verified),
                );
            })
            .map_err(ProtocolFailure::from),
        Request::Hello { .. } | Request::Cancel { .. } | Request::Shutdown => return,
    };
    if let Err(error) = result {
        send(
            responses,
            ResponseEnvelope::error(request_id, error.code, error.message),
        );
    }
}

fn index_source(
    index: &mut SearchIndex,
    responses: &mpsc::Sender<ResponseEnvelope>,
    request_id: &str,
    path: PathBuf,
    cancellation: &AtomicBool,
) -> Result<(), ProtocolFailure> {
    if cancellation.load(Ordering::Relaxed) {
        return Err(IndexError::Cancelled.into());
    }
    let loaded = OneNoteLoader::default()
        .load(path)
        .map_err(|error| ProtocolFailure {
            code: "source_error",
            message: error.to_string(),
        })?;
    let source_id = loaded.notebook.source_id.clone();
    index
        .replace_source(&loaded.notebook, cancellation, |progress| {
            send(
                responses,
                ResponseEnvelope::new(request_id, Response::Progress { progress }),
            );
        })
        .map_err(ProtocolFailure::from)?;
    let source = find_source(index, &source_id).map_err(ProtocolFailure::from)?;
    send(
        responses,
        ResponseEnvelope::new(request_id, Response::Indexed { source }),
    );
    Ok(())
}

fn find_source(
    index: &SearchIndex,
    source_id: &SourceId,
) -> onenote_index::Result<onenote_index::SourceStatus> {
    index
        .sources()?
        .into_iter()
        .find(|source| &source.source_id == source_id)
        .ok_or_else(|| IndexError::Database {
            message: "published source status is missing".to_owned(),
        })
}

fn send_result_batches(
    responses: &mpsc::Sender<ResponseEnvelope>,
    request_id: &str,
    hits: Vec<onenote_index::SearchHit>,
) {
    if hits.is_empty() {
        send(
            responses,
            ResponseEnvelope::new(
                request_id,
                Response::Results {
                    hits,
                    complete: true,
                },
            ),
        );
        return;
    }
    let batch_count = hits.len().div_ceil(RESULT_BATCH_SIZE);
    let mut iterator = hits.into_iter();
    for batch_index in 0..batch_count {
        let batch: Vec<_> = iterator.by_ref().take(RESULT_BATCH_SIZE).collect();
        send(
            responses,
            ResponseEnvelope::new(
                request_id,
                Response::Results {
                    hits: batch,
                    complete: batch_index + 1 == batch_count,
                },
            ),
        );
    }
}

fn write_responses(receiver: mpsc::Receiver<ResponseEnvelope>) -> io::Result<()> {
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());
    for response in receiver {
        serde_json::to_writer(&mut writer, &response)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    Ok(())
}

fn send(sender: &mpsc::Sender<ResponseEnvelope>, response: ResponseEnvelope) {
    let _ignored = sender.send(response);
}

fn lock_cancellations(
    cancellations: &CancellationMap,
) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
    cancellations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct ProtocolFailure {
    code: &'static str,
    message: String,
}

impl From<IndexError> for ProtocolFailure {
    fn from(error: IndexError) -> Self {
        Self {
            code: index_error_code(&error),
            message: error.to_string(),
        }
    }
}

fn index_error_code(error: &IndexError) -> &'static str {
    match error {
        IndexError::Database { .. } => "database_error",
        IndexError::IncompatibleSchema { .. } => "incompatible_schema",
        IndexError::InvalidQuery { .. } => "invalid_query",
        IndexError::Cancelled => "cancelled",
        IndexError::ResultLimit { .. } => "result_limit",
    }
}

fn database_argument() -> Result<PathBuf, &'static str> {
    let mut arguments = env::args_os().skip(1);
    match (arguments.next(), arguments.next(), arguments.next()) {
        (Some(flag), Some(path), None) if flag == "--database" => Ok(PathBuf::from(path)),
        _ => Err("usage: onenote-query --database PATH"),
    }
}
