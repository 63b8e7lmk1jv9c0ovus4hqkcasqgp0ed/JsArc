//! This is a Send + Sync abstraction for JsValue.
//!
//! It makes it possible to wrap a JsValue in an Arc and use it with other wrapped JsValues. It also supports async closures and covers some trait operations. **It accomplishes this by keeping JsValues on one thread and sending closures from other threads to be executed on it**.
//!
//!
//! This is how to create a wrapped value
//!```
//!let js_v = JsArc::new(|| "Hello World!".into()).await;
//!
//!js_v.with_self(|js_value| {
//!    web_sys::console::log_1(&js_value);
//!    js_value
//!})
//!.await;
//!```
//! After creating values, they can still be changed
//!```
//!let js_v = JsArc::new(|| 2.into()).await;
//!js_v.with_self(|one| one + &5.into()).await;
//!
//!// Outputs 7
//!js_v.with_self(|js_v| {
//!    web_sys::console::log_1(&js_v);
//!    js_v
//!})
//!.await;
//! ```
//!
//!
//! This demonstrates creating two JsValues and using them with each other
//! ```
//!let one = JsArc::new(|| 1.into()).await;
//!let two = JsArc::new(|| 2.into()).await;
//!
//!let three = one
//!    .with_other(&two, |one, two| {
//!        let add_result: JsValue = &one + &two;
//!
//!        (one, two, add_result)
//!    })
//!    .await;
//!
//!three
//!    .with_self(|three| {
//!        web_sys::console::log_1(&three);
//!
//!        three
//!    })
//!    .await;
//!```
//!   
//!It also works with async closures
//!   
//!```
//!let js_v = JsArc::new(|| "Hello World!".into()).await;
//!
//!js_v.with_self_async(|hello| async move {
//!    web_sys::console::log_1(&"Waiting 5 second then printing value".into());
//!    async_std::task::sleep(Duration::from_secs(5)).await;
//!    web_sys::console::log_1(&hello);
//!
//!    hello
//!})
//!.await;
//!```
//!    
//!And async closures with two JsValues
//!    
//!```
//!let five = three
//!    .with_other_async(&two, |three, two| async {
//!        web_sys::console::log_1(&"Waiting 1 second then adding values".into());
//!        async_std::task::sleep(Duration::from_secs(1)).await;
//!        let add_result: JsValue = &three + &two;
//!
//!        (three, two, add_result)
//!    })
//!    .await;
//!
//!five.with_self(|five| {
//!    web_sys::console::log_1(&five);
//!
//!    five
//!})
//!.await;
//!```
//!And many JsValues
//!```
//!let v0 = JsArc::new(|| 0.into()).await;
//!let v1 = JsArc::new(|| 1.into()).await;
//!let v2 = JsArc::new(|| 2.into()).await;
//!let v3 = JsArc::new(|| 3.into()).await;
//!let v4 = JsArc::new(|| 4.into()).await;
//!
//!let ten = v0
//!    .with_many(vec![&v1, &v2, &v3, &v4], |mut all| {
//!        let [v0, v1, v2, v3, v4] = &all[..] else {
//!            unreachable!("Not possible");
//!        };
//!
//!        let result = v0 + v1 + v2 + v3 + v4;
//!
//!        all.push(result);
//!        all
//!    })
//!    .await; 
//!```
//!The purpose for writing it was to simplify use of JsValues across different threads.
//!    
//!```
//!// It works inside Arc so it can be used on different threads. JsValue does not support Send + Sync by itself.
//!let value_in_arc: Arc<JsArc> = Arc::new(three);
//!let str: Arc<JsArc> = JsArc::new(|| "hello!".into()).await.arc();
//!```
//! And it also supports some trait operations: Add, BitAnd, BitOr, BitXor, Div, Mul, Neg, Not, Shl, Shr, Sub
//!```
//!let three = (&one + &two).await;
//!let ten = (&(&three * &three).await + &one).await;
//!let seven = (&ten - &three).await;
//!
//!seven
//!    .with_self(|seven| {
//!        web_sys::console::log_1(&seven);
//!
//!        seven
//!    })
//!    .await;
//!```
//! # Warning
//! <div class="warning">This is not tested. The syntax of this abstraction compiles, but it has not been tested with actual multithreading. Rust WASM does not support threading by default in the 2021 edition. In this untested state it can be used to pass trait restriction requirements for Send + Sync. There was not unsafe code used in writing this and the implmentation should be correct for a threading environment.
//! </div>

use std::{
    collections::HashMap,
    ops::{Add, BitAnd, BitOr, BitXor, Div, Mul, Neg, Not, Shl, Shr, Sub},
    pin::Pin,
    sync::{Arc, Mutex},
};

use async_channel::{unbounded, Receiver, Sender};

use once_cell::sync::Lazy;
use std::future::Future;

use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{js_sys::Object, wasm_bindgen};
trait ExecuteOnJs: Send + Sync {
    fn execute(&self, v: JsValue) -> JsValue;
}

impl<T: Fn(JsValue) -> JsValue + Send + Sync> ExecuteOnJs for T {
    fn execute(&self, v: JsValue) -> JsValue {
        (&self)(v)
    }
}

trait ExecuteOnMultiple: Send + Sync {
    fn execute(&self, v: JsValue, v2: JsValue) -> (JsValue, JsValue, JsValue);
}

impl<T: Fn(JsValue, JsValue) -> (JsValue, JsValue, JsValue) + Send + Sync> ExecuteOnMultiple for T {
    fn execute(&self, v: JsValue, v2: JsValue) -> (JsValue, JsValue, JsValue) {
        (&self)(v, v2)
    }
}

trait ExecuteOnMany: Send + Sync {
    fn execute(&self, v: Vec<JsValue>) -> Vec<JsValue>;
}

impl<T: Fn(Vec<JsValue>) -> Vec<JsValue> + Send + Sync> ExecuteOnMany for T {
    fn execute(&self, v: Vec<JsValue>) -> Vec<JsValue> {
        (&self)(v)
    }
}

trait ExecuteOnManyAsync: Send + Sync + 'static {
    fn execute(&self, v: Vec<JsValue>) -> Pin<Box<dyn Future<Output = Vec<JsValue>>>>;
}

impl<
        FUT: Future<Output = Vec<JsValue>> + 'static,
        T: Fn(Vec<JsValue>) -> FUT + Send + Sync + 'static,
    > ExecuteOnManyAsync for T
{
    fn execute(&self, v: Vec<JsValue>) -> Pin<Box<dyn Future<Output = Vec<JsValue>>>> {
        Box::pin((&self)(v))
    }
}

trait ExecuteOnJsAsync: Send + Sync + 'static {
    fn execute(&self, v: JsValue) -> Pin<Box<dyn Future<Output = JsValue>>>;
}

impl<R: Future<Output = JsValue> + 'static, F: Fn(JsValue) -> R + Send + Sync + 'static>
    ExecuteOnJsAsync for F
{
    fn execute(&self, v: JsValue) -> Pin<Box<dyn Future<Output = JsValue>>> {
        Box::pin((&self)(v))
    }
}

trait ExecuteOnMultipleAsync: Send + Sync + 'static {
    fn execute(
        &self,
        v1: JsValue,
        v2: JsValue,
    ) -> Pin<Box<dyn Future<Output = (JsValue, JsValue, JsValue)>>>;
}

impl<
        R: Future<Output = (JsValue, JsValue, JsValue)> + 'static,
        F: Fn(JsValue, JsValue) -> R + Send + Sync + 'static,
    > ExecuteOnMultipleAsync for F
{
    fn execute(
        &self,
        v1: JsValue,
        v2: JsValue,
    ) -> Pin<Box<dyn Future<Output = (JsValue, JsValue, JsValue)>>> {
        Box::pin((&self)(v1, v2))
    }
}

/// This handle provides access to the running JsArcServer which maintains the storage for all JsValues
/// that are being managed by JsArc values.
///
/// If the value of it is None, it hasn't been initialized yet.
static JS_ARC_HANDLE: Lazy<Mutex<Option<Arc<async_channel::Sender<ServerCommands>>>>> =
    Lazy::new(|| Mutex::new(None));

enum ServerCommands {
    // function, result_id
    Initialize(Box<dyn ExecuteOnJs>, Sender<usize>),
    InitializeAsync(Box<dyn ExecuteOnJsAsync>, Sender<usize>),
    // id, function, notify when done executing
    Use(usize, Box<dyn ExecuteOnJs>, Sender<()>),
    UseAsync(usize, Box<dyn ExecuteOnJsAsync>, Sender<()>),
    // id1, id2, function, result_id
    UseTwo(usize, usize, Box<dyn ExecuteOnMultiple>, Sender<usize>),
    UseTwoAsync(usize, usize, Box<dyn ExecuteOnMultipleAsync>, Sender<usize>),
    // id of self, id for each item in vec, closure, result_id for new value
    UseMany(usize, Vec<usize>, Box<dyn ExecuteOnMany>, Sender<usize>),
    UseManyAsync(
        usize,
        Vec<usize>,
        Box<dyn ExecuteOnManyAsync>,
        Sender<usize>,
    ),

    // Remove value with id from memory
    Deallocate(usize),
    // The id maps to a complete handle that is used to notify the creating client that the future is processed
    AsyncExecutionComplete(usize),
}

struct ExecutionResult<IDS, SENDBACKTYPE, SAVERESULT> {
    // The send back type is what should be sent to the JsArc abstraction
    js_arc_notifier: Sender<SENDBACKTYPE>,
    // These are the ids in memory that correspond to JsValues. These need to be used to save the `result`
    save_location_ids: IDS,
    // This is the resulting JsValues after values from memory or used or created.
    result: SAVERESULT,
}

enum FinishAsyncContainer {
    // newly issued id, send back the new id for the JsArc, the JsValue that needs to be saved at the newly issued id
    AfterCreateSendId(Receiver<ExecutionResult<usize, usize, JsValue>>),
    // locations in memory that correspond to the js values. The newly issued id that the abstraction needs, the values to save to memory in the 1 fields memory locations
    AfterMultiSaveCreateSendId(
        Receiver<ExecutionResult<(usize, usize, usize), usize, (JsValue, JsValue, JsValue)>>,
    ),
    AfterManySaveCreateSendId(Receiver<ExecutionResult<Vec<usize>, usize, Vec<JsValue>>>),
    // save the JsValue at the usize location, send the JsArc a complete signal when done
    AfterUseSaveResultAndNotify(Receiver<ExecutionResult<usize, (), JsValue>>),
}

struct JsArcServer {
    storage: HashMap<usize, JsValue>,
    issued: usize,
    // This is for async tasks to use while they're being processed. This id
    // is for async_issued, not for issued.
    temp_async_storage: HashMap<usize, FinishAsyncContainer>,
    async_issued: usize,
    handle: async_channel::Receiver<ServerCommands>,
}

impl JsArcServer {
    fn claim_id(&mut self) -> usize {
        let id = self.issued;
        self.issued += 1;
        id
    }

    fn claim_async_id(&mut self) -> usize {
        let id = self.async_issued;
        self.async_issued += 1;
        id
    }

    async fn run(&mut self) {
        while let Ok(next_cmd) = self.handle.recv().await {
            match next_cmd {
                ServerCommands::Initialize(create_closure, after_create_sender) => {
                    let undefined: JsValue = Object::new().into();

                    let id = self.claim_id();

                    let initial = create_closure.execute(undefined);
                    self.storage.insert(id, initial);

                    let _ = after_create_sender.send(id).await;
                }
                ServerCommands::InitializeAsync(create_closure, after_create_sender) => {
                    let undefined: JsValue = Object::new().into();

                    let id = self.claim_id();
                    let async_id = self.claim_async_id();

                    let (s, r) = unbounded();

                    self.temp_async_storage
                        .insert(async_id, FinishAsyncContainer::AfterCreateSendId(r));

                    // The goal is to not block receiving messages while async messages are processing. As an example,
                    // the executing async function could timeout for 10 minutes. If this waited 10 minutes,
                    // to handle the next sent server command, it really wouldn't be working as expected.
                    wasm_bindgen_futures::spawn_local(async move {
                        let initial = create_closure.execute(undefined).await;

                        let result = ExecutionResult {
                            js_arc_notifier: after_create_sender,
                            save_location_ids: id,
                            result: initial,
                        };

                        let _ = s.send(result).await;

                        let client = JsArcClient::new();

                        let _ = client
                            .handle
                            .send(ServerCommands::AsyncExecutionComplete(async_id))
                            .await;
                    });
                }
                ServerCommands::Use(id, use_value_closure, after_notify) => {
                    let from_storage: JsValue = self.storage.get(&id).unwrap().clone();

                    let after_use: JsValue = use_value_closure.execute(from_storage);
                    self.storage.insert(id, after_use);

                    let _ = after_notify.send(()).await;
                }

                // This is an almost word for word copy of the above, I just found it difficult to restructure
                ServerCommands::UseAsync(id, use_value_closure, after_notify) => {
                    let from_storage: JsValue = self.storage.get(&id).unwrap().clone();
                    let async_id = self.claim_async_id();

                    let (s, r) = unbounded();

                    self.temp_async_storage.insert(
                        async_id,
                        FinishAsyncContainer::AfterUseSaveResultAndNotify(r),
                    );

                    // The goal is to not block receiving messages while async messages are processing. As an example,
                    // the executing async function could timeout for 10 minutes. If this waited 10 minutes,
                    // to handle the next sent server command, it really wouldn't be working as expected.
                    wasm_bindgen_futures::spawn_local(async move {
                        let result_after_use = use_value_closure.execute(from_storage).await;

                        let result = ExecutionResult {
                            js_arc_notifier: after_notify,
                            save_location_ids: id,
                            result: result_after_use,
                        };

                        let _ = s.send(result).await;

                        let client = JsArcClient::new();

                        let _ = client
                            .handle
                            .send(ServerCommands::AsyncExecutionComplete(async_id))
                            .await;
                    });
                }
                ServerCommands::Deallocate(id) => {
                    self.storage.remove(&id);
                }
                ServerCommands::UseTwo(id1, id2, with_two_closure, result_id_sender) => {
                    let from_storage_one: JsValue = self.storage.get(&id1).unwrap().clone();
                    let from_storage_two: JsValue = self.storage.get(&id2).unwrap().clone();

                    let (after_one, after_two, result) =
                        with_two_closure.execute(from_storage_one, from_storage_two);

                    self.storage.insert(id1, after_one);
                    self.storage.insert(id2, after_two);
                    self.storage.insert(self.issued, result);

                    let result_id = self.claim_id();

                    let _ = result_id_sender.send(result_id).await;
                }

                // This is an almost word for word copy of the above, I just found it difficult to restructure
                ServerCommands::UseTwoAsync(id1, id2, with_two_closure, result_id_sender) => {
                    let from_storage_one: JsValue = self.storage.get(&id1).unwrap().clone();
                    let from_storage_two: JsValue = self.storage.get(&id2).unwrap().clone();

                    let async_id = self.claim_async_id();
                    let id = self.claim_id();

                    let (s, r) = unbounded();

                    self.temp_async_storage.insert(
                        async_id,
                        FinishAsyncContainer::AfterMultiSaveCreateSendId(r),
                    );

                    // The goal is to not block receiving messages while async messages are processing. As an example,
                    // the executing async function could timeout for 10 minutes. If this waited 10 minutes,
                    // to handle the next sent server command, it really wouldn't be working as expected.
                    wasm_bindgen_futures::spawn_local(async move {
                        let save = (id1, id2, id);
                        let after = with_two_closure
                            .execute(from_storage_one, from_storage_two)
                            .await;

                        let result = ExecutionResult {
                            js_arc_notifier: result_id_sender,
                            save_location_ids: save,
                            result: after,
                        };

                        let _ = s.send(result).await;

                        let client = JsArcClient::new();

                        let _ = client
                            .handle
                            .send(ServerCommands::AsyncExecutionComplete(async_id))
                            .await;
                    });
                }

                ServerCommands::UseMany(id, mut other_ids, to_execute, extra_save_value) => {
                    other_ids.insert(0, id);
                    let mut all_ids = other_ids;

                    let from_memory: Vec<JsValue> = all_ids
                        .iter()
                        .map(|v| self.storage.get(v).unwrap().clone())
                        .collect();

                    let after = to_execute.execute(from_memory);

                    let last = self.claim_id();
                    all_ids.push(last);

                    all_ids
                        .iter()
                        .zip(after)
                        .map(|(k, v)| {
                            self.storage.insert(*k, v);
                        })
                        .count();

                    let _ = extra_save_value.send(last).await;
                }
                ServerCommands::UseManyAsync(id, mut other_ids, to_execute, extra_save_value) => {
                    let store_async_id = self.claim_async_id();
                    let new_id = self.claim_id();

                    other_ids.insert(0, id);
                    let mut all_ids = other_ids;

                    let from_memory: Vec<JsValue> = all_ids
                        .iter()
                        .map(|v| self.storage.get(v).unwrap().clone())
                        .collect();

                    all_ids.push(new_id);

                    let (s, r) = unbounded();

                    self.temp_async_storage.insert(
                        store_async_id,
                        FinishAsyncContainer::AfterManySaveCreateSendId(r),
                    );

                    wasm_bindgen_futures::spawn_local(async move {
                        let save = all_ids;
                        let after = to_execute.execute(from_memory).await;

                        let result = ExecutionResult {
                            js_arc_notifier: extra_save_value,
                            save_location_ids: save,
                            result: after,
                        };

                        let _ = s.send(result).await;

                        let client = JsArcClient::new();

                        let _ = client
                            .handle
                            .send(ServerCommands::AsyncExecutionComplete(store_async_id))
                            .await;
                    });
                    // );
                }
                ServerCommands::AsyncExecutionComplete(async_id) => {
                    let result = self
                        .temp_async_storage
                        .remove(&async_id)
                        .expect("I don't know how it was removed, this should never happen");

                    match result {
                        FinishAsyncContainer::AfterCreateSendId(recv) => {
                            let result = recv.recv().await.expect("This should never fail");

                            let ExecutionResult {
                                js_arc_notifier,
                                save_location_ids: id,
                                result,
                            } = result;

                            self.storage.insert(id, result);

                            let _ = js_arc_notifier.send(id).await;
                        }
                        FinishAsyncContainer::AfterUseSaveResultAndNotify(recv) => {
                            let result = recv.recv().await.expect("This should never fail");

                            let ExecutionResult {
                                js_arc_notifier,
                                save_location_ids: id,
                                result,
                            } = result;

                            self.storage.insert(id, result);

                            let _ = js_arc_notifier.send(()).await;
                        }
                        FinishAsyncContainer::AfterMultiSaveCreateSendId(recv) => {
                            let result = recv.recv().await.expect("This should never fail");

                            let ExecutionResult {
                                js_arc_notifier,
                                save_location_ids: (id1, id2, newly_issued_id),
                                result: (after_one, after_two, result),
                            } = result;

                            self.storage.insert(id1, after_one);
                            self.storage.insert(id2, after_two);
                            self.storage.insert(newly_issued_id, result);

                            let _ = js_arc_notifier.send(newly_issued_id).await;
                        }
                        FinishAsyncContainer::AfterManySaveCreateSendId(recv) => {
                            let result = recv.recv().await.expect("This should never fail");

                            let ExecutionResult {
                                js_arc_notifier,
                                save_location_ids,
                                result,
                            } = result;

                            let last_copy = *save_location_ids.last().unwrap();

                            save_location_ids
                                .into_iter()
                                .zip(result.into_iter())
                                .for_each(|(k, v)| {
                                    self.storage.insert(k, v);
                                });

                            let _ = js_arc_notifier.send(last_copy).await;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct JsArcClient {
    handle: Arc<async_channel::Sender<ServerCommands>>,
}

impl JsArcClient {
    fn new() -> JsArcClient {
        if let Ok(mut locked) = JS_ARC_HANDLE.lock() {
            let handle: &mut Option<Arc<Sender<ServerCommands>>> = &mut locked;

            if handle.is_none() {
                let (s, r) = async_channel::unbounded::<ServerCommands>();

                let server = JsArcServer {
                    storage: HashMap::new(),
                    issued: 0,
                    handle: r,
                    temp_async_storage: HashMap::new(),
                    async_issued: 0,
                };

                *handle = Some(Arc::new(s));

                wasm_bindgen_futures::spawn_local(async move {
                    let mut server = server;

                    server.run().await;
                });
            }

            if let Some(initalized_handle) = handle {
                let borrowed_handle: &mut Arc<_> = initalized_handle;

                let copy = borrowed_handle.clone();

                let client = JsArcClient { handle: copy };

                return client;
            }
        }

        panic!("It should not be possible to panic. It's possible there's a problem with the Mutex implementation used in the environment.");
    }
}

pub struct JsArc {
    client: JsArcClient,
    issue_id: usize,
}

impl JsArc {
    /// Creates a wrapped JsValue from an initialization closure.
    ///```
    ///let js_value = JsArc::new(|| "Hello World!".into()).await;
    ///```
    pub async fn new<F: Fn() -> JsValue + Send + Sync + 'static>(
        initialization_closure: F,
    ) -> JsArc {
        let initalize_fn = move |_| initialization_closure();
        let client = JsArcClient::new();

        let (s, r) = unbounded();

        let _ = client
            .handle
            .send(ServerCommands::Initialize(Box::new(initalize_fn), s))
            .await;

        let id = r.recv().await.unwrap();

        JsArc {
            client,
            issue_id: id,
        }
    }

    /// Creates a wrapped JsValue from an async initialization closure.
    ///```
    ///#[wasm_bindgen]
    ///extern "C" {
    ///    #[wasm_bindgen]
    ///    pub type HelloWorldModule;
    ///
    ///    #[wasm_bindgen(method, js_name = "helloWorld")]
    ///    pub fn hello_world(this: &HelloWorldModule);
    ///
    ///}
    ///
    ///pub async fn load_dynamic_hello_world() -> HelloWorldModule {
    ///    let module_as_str = r#"export function helloWorld() { console.log("Hello World!"); }"#;
    ///
    ///    let from_data = Array::new();
    ///    from_data.push(&module_as_str.into());
    ///
    ///    let mut type_set: BlobPropertyBag = BlobPropertyBag::new();
    ///    type_set.type_("application/javascript");
    ///
    ///    let blob = Blob::new_with_str_sequence_and_options(&from_data, &type_set).unwrap();
    ///    let module_address = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    ///
    ///    let module_promise: Promise = js_sys::eval(&format!(r#"import ("{}")"#, module_address))
    ///        .unwrap()
    ///        .into();
    ///
    ///    let module = JsFuture::from(module_promise).await.unwrap();
    ///
    ///    let as_hello_world: HelloWorldModule = module.into();
    ///    as_hello_world.hello_world();
    ///
    ///    as_hello_world
    ///}
    ///
    ///let module = JsArc::new_async(|| async { load_dynamic_hello_world().await.into() }).await;
    ///```
    pub async fn new_async<
        K: Future<Output = JsValue> + 'static,
        F: Fn() -> K + Send + Sync + 'static,
    >(
        initialization_closure: F,
    ) -> JsArc {
        let initalize_fn = move |_| {
            let generation = initialization_closure();

            async { generation.await }
        };

        let client = JsArcClient::new();

        let (s, r) = unbounded();

        let _ = client
            .handle
            .send(ServerCommands::InitializeAsync(Box::new(initalize_fn), s))
            .await;

        let id = r.recv().await.unwrap();

        JsArc {
            client,
            issue_id: id,
        }
    }

    /// Wraps in an Arc. The impl is the following
    ///```
    ///Arc::new(self)
    ///```
    pub fn arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Provides the ability to use and modify the JsValue abstracted by the wrapped JsValue
    /// ```
    ///let js_v = JsArc::new(|| "Hello World!".into()).await;
    ///
    ///js_v.with_self(|js_value| {
    ///    web_sys::console::log_1(&js_value);
    ///    js_value
    ///})
    ///.await;
    ///```
    /// The value returned from the closure is the value saved for the abstraction.
    ///
    /// The following outputs 7
    /// ```
    ///let js_v = JsArc::new(|| 2.into()).await;
    ///js_v.with_self(|one| one + &5.into()).await;
    ///js_v.with_self(|js_v| {
    ///    web_sys::console::log_1(&js_v);
    ///    js_v
    ///})
    ///.await;
    ///```
    pub async fn with_self<F: Fn(JsValue) -> JsValue + Send + Sync + 'static>(
        &self,
        usage_closure: F,
    ) {
        let id = self.issue_id.clone();
        let client = self.client.clone();

        let (s, r) = unbounded();

        let _ = client
            .handle
            .send(ServerCommands::Use(id, Box::new(usage_closure), s))
            .await;

        let _ = r.recv().await;
    }

    /// Provides the ability to asynchronously use and modify the JsValue abstracted by the wrapped JsValue
    ///```
    ///js_value
    ///    .with_self_async(|hello| async move {
    ///        web_sys::console::log_1(&"Waiting 5 second then printing value".into());
    ///        async_std::task::sleep(Duration::from_secs(5)).await;
    ///        web_sys::console::log_1(&hello);
    ///
    ///        hello
    ///    })
    ///    .await;
    ///```
    pub async fn with_self_async<
        R: Future<Output = JsValue> + 'static,
        F: Fn(JsValue) -> R + Send + Sync + 'static,
    >(
        &self,
        v: F,
    ) {
        let id = self.issue_id.clone();
        let client = self.client.clone();

        let (s, r) = unbounded();

        let _ = client
            .handle
            .send(ServerCommands::UseAsync(id, Box::new(v), s))
            .await;

        let _ = r.recv().await;
    }

    /// Provides the ability to use and modify two JsValues at once while also creating a new abstracted JsValue for a third value
    ///```
    ///let one = JsArc::new(|| 1.into()).await;
    ///let two = JsArc::new(|| 2.into()).await;
    ///
    ///let three = one
    ///    .with_other(&two, |one, two| {
    ///        let add_result: JsValue = &one + &two;
    ///
    ///        (one, two, add_result)
    ///    })
    ///    .await;
    ///    
    ///three
    ///    .with_self(|three| {
    ///        web_sys::console::log_1(&three);
    ///
    ///        three
    ///    })
    ///    .await;
    ///```
    pub async fn with_other<
        F: Fn(JsValue, JsValue) -> (JsValue, JsValue, JsValue) + Send + Sync + 'static,
    >(
        &self,
        other: &Self,
        v: F,
    ) -> JsArc {
        let id1 = self.issue_id.clone();
        let id2 = other.issue_id.clone();
        let client = self.client.clone();

        let (s, r) = unbounded();

        let _ = client
            .handle
            .send(ServerCommands::UseTwo(id1, id2, Box::new(v), s))
            .await;

        let result = JsArc {
            client: client.clone(),
            issue_id: r.recv().await.unwrap(),
        };

        result
    }

    /// Provides the ability to asynchronously use and modify two JsValues at once while also creating a new abstracted JsValue for a third value
    ///```
    ///let five = three
    ///    .with_other_async(&two, |three, two| async {
    ///        web_sys::console::log_1(&"Waiting 1 second then adding values".into());
    ///        async_std::task::sleep(Duration::from_secs(1)).await;
    ///        let add_result: JsValue = &three + &two;
    ///
    ///        (three, two, add_result)
    ///    })
    ///    .await;
    ///
    ///five.with_self(|five| {
    ///    web_sys::console::log_1(&five);
    ///
    ///    five
    ///})
    ///.await;
    ///```
    pub async fn with_other_async<
        R: Future<Output = (JsValue, JsValue, JsValue)> + 'static,
        F: Fn(JsValue, JsValue) -> R + Send + Sync + 'static,
    >(
        &self,
        other: &Self,
        v: F,
    ) -> JsArc {
        let id1 = self.issue_id.clone();
        let id2 = other.issue_id.clone();
        let client = self.client.clone();

        let (s, r) = unbounded();

        let _ = client
            .handle
            .send(ServerCommands::UseTwoAsync(id1, id2, Box::new(v), s))
            .await;

        let result = JsArc {
            client: client.clone(),
            issue_id: r.recv().await.unwrap(),
        };

        result
    }

    /// Places the &self value in the front of the Vec, then calls the defined closure. It expects
    /// to end with an extra value at the end of the Vec.
    ///```
    ///let v0 = JsArc::new(|| 0.into()).await;
    ///let v1 = JsArc::new(|| 1.into()).await;
    ///let v2 = JsArc::new(|| 2.into()).await;
    ///let v3 = JsArc::new(|| 3.into()).await;
    ///let v4 = JsArc::new(|| 4.into()).await;
    ///
    ///let ten = v0
    ///    .with_many(vec![&v1, &v2, &v3, &v4], |mut all| {
    ///        let [v0, v1, v2, v3, v4] = &all[..] else {
    ///            unreachable!("Not possible");
    ///        };
    ///
    ///        let result = v0 + v1 + v2 + v3 + v4;
    ///
    ///        all.push(result);
    ///        all
    ///    })
    ///    .await;
    ///```  
    pub async fn with_many<F: Fn(Vec<JsValue>) -> Vec<JsValue> + Send + Sync + 'static>(
        &self,
        others: Vec<&Self>,
        closure_all_returns_all_and_extra: F,
    ) -> JsArc {
        let id1 = self.issue_id.clone();
        let others: Vec<usize> = others.iter().map(|v| v.issue_id.clone()).collect();

        let client = self.client.clone();

        let (s, r) = unbounded();

        let _ = client
            .handle
            .send(ServerCommands::UseMany(
                id1,
                others,
                Box::new(closure_all_returns_all_and_extra),
                s,
            ))
            .await;

        let result = JsArc {
            client: client.clone(),
            issue_id: r.recv().await.unwrap(),
        };

        result
    }

    /// Places the &self value in the front of the Vec, then calls the defined closure. It expects
    /// to end with an extra value at the end of the Vec.
    ///```
    ///    let v0 = JsArc::new(|| 0.into()).await;
    ///    let v1 = JsArc::new(|| 1.into()).await;
    ///    let v2 = JsArc::new(|| 2.into()).await;
    ///    let v3 = JsArc::new(|| 3.into()).await;
    ///    let v4 = JsArc::new(|| 4.into()).await;
    ///
    ///    let twenty = v0
    ///        .with_many_async(vec![&v1, &v2, &v3, &v4], |mut all| async {
    ///            let [v0, v1, v2, v3, v4] = &all[..] else {
    ///                unreachable!("Not possible");
    ///            };
    ///
    ///            let result = (v0 + v1 + v2 + v3 + v4) * v2;
    ///
    ///            web_sys::console::log_1(&"Waiting 1 second before continuing".into());
    ///            async_std::task::sleep(Duration::from_secs(1)).await;
    ///
    ///            all.push(result);
    ///            all
    ///        })
    ///        .await;
    ///
    ///    twenty
    ///        .with_self(|result| {
    ///            web_sys::console::log_1(&result);
    ///
    ///            result
    ///        })
    ///        .await;    
    ///```
    pub async fn with_many_async<
        FUT: Future<Output = Vec<JsValue>> + 'static,
        F: Fn(Vec<JsValue>) -> FUT + Send + Sync + 'static,
    >(
        &self,
        others: Vec<&Self>,
        closure_all_returns_all_and_extra: F,
    ) -> JsArc {
        let id1 = self.issue_id.clone();
        let others: Vec<usize> = others.iter().map(|v| v.issue_id.clone()).collect();

        let client = self.client.clone();

        let (s, r) = unbounded();

        let _ = client
            .handle
            .send(ServerCommands::UseManyAsync(
                id1,
                others,
                Box::new(closure_all_returns_all_and_extra),
                s,
            ))
            .await;

        let result = JsArc {
            client: client.clone(),
            issue_id: r.recv().await.unwrap(),
        };

        result
    }
}

impl Drop for JsArc {
    fn drop(&mut self) {
        let drop_id = self.issue_id.clone();
        let handle_copy = self.client.handle.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let _ = handle_copy.send(ServerCommands::Deallocate(drop_id)).await;
        });
    }
}

pub struct JsArcFutureWrap {
    inner: Option<Pin<Box<dyn Future<Output = JsArc>>>>,
}

impl Future for JsArcFutureWrap {
    type Output = JsArc;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let Some(mut fut) = self.inner.take() else {
            unreachable!("Not possible");
        };

        let state = Future::poll(fut.as_mut(), cx);

        self.inner = Some(fut);

        state
    }
}

pub struct JsArcFutureWrapLifetime<'a> {
    inner: Option<Pin<Box<dyn Future<Output = JsArc> + 'a>>>,
    // other: PhantomData<&'a A>,
}

impl<'a> Future for JsArcFutureWrapLifetime<'a> {
    type Output = JsArc;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let Some(mut fut) = self.inner.take() else {
            unreachable!("Not possible");
        };

        let state = Future::poll(fut.as_mut(), cx);

        self.inner = Some(fut);

        state
    }
}

impl Add for JsArc {
    type Output = JsArcFutureWrap;

    fn add(self, rhs: Self) -> Self::Output {
        let out = async move {
            self.with_other(&rhs, |l, r| {
                let result = &l + &r;
                (l, r, result)
            })
            .await
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl BitAnd for JsArc {
    type Output = JsArcFutureWrap;

    fn bitand(self, rhs: Self) -> Self::Output {
        let out = async move {
            self.with_other(&rhs, |l, r| {
                let result = &l & &r;
                (l, r, result)
            })
            .await
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl BitOr for JsArc {
    type Output = JsArcFutureWrap;

    fn bitor(self, rhs: Self) -> Self::Output {
        let out = async move {
            self.with_other(&rhs, |l, r| {
                let result = &l | &r;
                (l, r, result)
            })
            .await
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl BitXor for JsArc {
    type Output = JsArcFutureWrap;

    fn bitxor(self, rhs: Self) -> Self::Output {
        let out = async move {
            self.with_other(&rhs, |l, r| {
                let result = &l ^ &r;
                (l, r, result)
            })
            .await
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl Div for JsArc {
    type Output = JsArcFutureWrap;

    fn div(self, rhs: Self) -> Self::Output {
        let out = async move {
            self.with_other(&rhs, |l, r| {
                let result = &l / &r;
                (l, r, result)
            })
            .await
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl Mul for JsArc {
    type Output = JsArcFutureWrap;

    fn mul(self, rhs: Self) -> Self::Output {
        let out = async move {
            self.with_other(&rhs, |l, r| {
                let result = &l * &r;

                (l, r, result)
            })
            .await
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl Neg for JsArc {
    type Output = JsArcFutureWrap;

    fn neg(self) -> Self::Output {
        let out = async move {
            let new = self
                .with_other(&JsArc::new(|| "undefined".into()).await, |l, _| {
                    let copy = l.clone();
                    let r: JsValue = (-copy).into();

                    (l, r.clone(), r)
                })
                .await;

            new
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl Not for JsArc {
    type Output = JsArcFutureWrap;

    fn not(self) -> Self::Output {
        let out = async move {
            let new = self
                .with_other(&JsArc::new(|| "undefined".into()).await, |l, _| {
                    let copy = l.clone();
                    let r: JsValue = (!copy).into();

                    (l, r.clone(), r)
                })
                .await;

            new
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl Shl for JsArc {
    type Output = JsArcFutureWrap;

    fn shl(self, rhs: Self) -> Self::Output {
        let out = async move {
            self.with_other(&rhs, |l, r| {
                let result = &l << &r;

                (l, r, result)
            })
            .await
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl Shr for JsArc {
    type Output = JsArcFutureWrap;

    fn shr(self, rhs: Self) -> Self::Output {
        let out = async move {
            self.with_other(&rhs, |l, r| {
                let result = &l >> &r;

                (l, r, result)
            })
            .await
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl Sub for JsArc {
    type Output = JsArcFutureWrap;

    fn sub(self, rhs: Self) -> Self::Output {
        let out = async move {
            self.with_other(&rhs, |l, r| {
                let result = &l - &r;

                (l, r, result)
            })
            .await
        };

        let result = JsArcFutureWrap {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> Add for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn add(self, rhs: Self) -> Self::Output {
        let out = self.with_other(&rhs, |l, r| {
            let result = &l + &r;

            (l, r, result)
        });

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> BitAnd for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn bitand(self, rhs: Self) -> Self::Output {
        let out = self.with_other(&rhs, |l, r| {
            let result = &l & &r;

            (l, r, result)
        });

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> BitOr for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn bitor(self, rhs: Self) -> Self::Output {
        let out = self.with_other(&rhs, |l, r| {
            let result = &l | &r;

            (l, r, result)
        });

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> BitXor for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn bitxor(self, rhs: Self) -> Self::Output {
        let out = self.with_other(&rhs, |l, r| {
            let result = &l ^ &r;

            (l, r, result)
        });

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> Div for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn div(self, rhs: Self) -> Self::Output {
        let out = self.with_other(&rhs, |l, r| {
            let result = &l / &r;

            (l, r, result)
        });

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> Mul for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn mul(self, rhs: Self) -> Self::Output {
        let out = self.with_other(&rhs, |l, r| {
            let result = &l * &r;

            (l, r, result)
        });

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> Neg for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn neg(self) -> Self::Output {
        let out = async move {
            let new = self
                .with_other(&JsArc::new(|| "undefined".into()).await, |l, _| {
                    let copy = l.clone();
                    let r: JsValue = (-copy).into();

                    (l, r.clone(), r)
                })
                .await;

            new
        };

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> Not for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn not(self) -> Self::Output {
        let out = async move {
            let new = self
                .with_other(&JsArc::new(|| "undefined".into()).await, |l, _| {
                    let copy = l.clone();
                    let r: JsValue = (!copy).into();

                    (l, r.clone(), r)
                })
                .await;

            new
        };

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> Shl for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn shl(self, rhs: Self) -> Self::Output {
        let out = self.with_other(&rhs, |l, r| {
            let result = &l << &r;

            (l, r, result)
        });

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> Shr for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn shr(self, rhs: Self) -> Self::Output {
        let out = self.with_other(&rhs, |l, r| {
            let result = &l >> &r;

            (l, r, result)
        });

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}

impl<'a> Sub for &'a JsArc {
    type Output = JsArcFutureWrapLifetime<'a>;

    fn sub(self, rhs: Self) -> Self::Output {
        let out = self.with_other(&rhs, |l, r| {
            let result = &l - &r;

            (l, r, result)
        });

        let result = JsArcFutureWrapLifetime {
            inner: Some(Box::pin(out)),
        };

        result
    }
}
