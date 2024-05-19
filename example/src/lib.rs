use std::time::Duration;

use js_arc::JsArc;
use wasm_bindgen::prelude::wasm_bindgen;
mod arc_js_module;
use arc_js_module::{load_dynamic_hello_world, HelloWorldModule};

#[wasm_bindgen]
pub async fn example() {
    let hello = JsArc::new(|| "Hello! The result is: ".into()).await.arc();

    let v1 = JsArc::new(|| 1.into()).await.arc();
    let v2 = JsArc::new(|| 2.into()).await.arc();
    let v3 = (&*v1 + &*v2).await.arc();

    let module = JsArc::new_async(|| async {
        let module = load_dynamic_hello_world().await.into();

        module
    })
    .await;

    module
        .with_self(|module| {
            let module: HelloWorldModule = module.into();
            module.hello_world();

            module.into()
        })
        .await;

    let concatenated = (&*hello + &*v3).await;

    concatenated
        .with_self(|result| {
            web_sys::console::log_1(&result);

            result
        })
        .await;

    let v0 = JsArc::new(|| 0.into()).await;
    let v1 = JsArc::new(|| 1.into()).await;
    let v2 = JsArc::new(|| 2.into()).await;
    let v3 = JsArc::new(|| 3.into()).await;
    let v4 = JsArc::new(|| 4.into()).await;

    let ten = v0
        .with_many(vec![&v1, &v2, &v3, &v4], |mut all| {
            let [v0, v1, v2, v3, v4] = &all[..] else {
                unreachable!("Not possible");
            };

            let result = v0 + v1 + v2 + v3 + v4;

            all.push(result);
            all
        })
        .await;

    ten.with_self(|result| {
        web_sys::console::log_1(&result);

        result
    })
    .await;

    let v0 = JsArc::new(|| 0.into()).await;
    let v1 = JsArc::new(|| 1.into()).await;
    let v2 = JsArc::new(|| 2.into()).await;
    let v3 = JsArc::new(|| 3.into()).await;
    let v4 = JsArc::new(|| 4.into()).await;

    let twenty = v0
        .with_many_async(vec![&v1, &v2, &v3, &v4], |mut all| async {
            let [v0, v1, v2, v3, v4] = &all[..] else {
                unreachable!("Not possible");
            };

            let result = (v0 + v1 + v2 + v3 + v4) * v2;

            web_sys::console::log_1(&"Waiting 1 second before continuing".into());
            async_std::task::sleep(Duration::from_secs(1)).await;

            all.push(result);
            all
        })
        .await;

    twenty
        .with_self(|result| {
            web_sys::console::log_1(&result);

            result
        })
        .await;

    send_sync_drop(concatenated);
}

// This is just to demonstrate that it's possible to use JsValue's even when trait requirements
// have Send + Sync defined.
fn send_sync_drop<V: Send + Sync>(_: V) {}
