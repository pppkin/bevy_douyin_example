//! 真机诊断，默认关闭。
//!
//! 在 `platforms/douyin/package/game.js` 里、`runtime.start(...)` 之前设置
//! `GameGlobal.__probe = 1`，重新生成预览后即可看到：
//!
//! * 前 6 帧每帧一条 `diag: update #n ...`，之后每 300 帧一条；
//! * `probe: on (...)`、`probe: canvas ...`、`probe: safe ...` 启动摘要；
//! * panic 时通过 `tt.showModal` 弹窗，而不是静默白屏。
//!
//! 为什么需要它：真机日志来自 wasm 内部时经常被 iOS 引擎丢掉，
//! `panic = "abort"` 也不会给 JS 栈；没有探针就只能看到白屏。

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::sync::atomic::{AtomicI8, Ordering};

static ENABLED: AtomicI8 = AtomicI8::new(-1);

/// 是否启用诊断（`GameGlobal.__probe`）。
///
/// 这里走 `eval` 而不是 `js_sys::global()`：抖音的 wasm host 里，JS 侧全局
/// 对象不一定是 `GameGlobal`，但 eval 一定运行在页面全局里。
pub fn enabled() -> bool {
    match ENABLED.load(Ordering::Relaxed) {
        1 => true,
        0 => false,
        _ => {
            let on = flag("__probe");
            ENABLED.store(if on { 1 } else { 0 }, Ordering::Relaxed);
            on
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn flag(name: &str) -> bool {
    js_sys::eval(&format!(
        "(typeof GameGlobal!=='undefined'&&GameGlobal.{name})||0"
    ))
    .ok()
    .and_then(|v| v.as_f64())
    .map(|f| f != 0.0)
    .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
fn flag(_name: &str) -> bool {
    false
}

/// 只有在探针打开时才写的 `info!`。
pub fn say(args: std::fmt::Arguments<'_>) {
    if enabled() {
        info!("{args}");
    }
}

#[cfg(target_arch = "wasm32")]
fn mem_mib() -> f64 {
    let mem = wasm_bindgen::memory();
    let buf = js_sys::Reflect::get(&mem, &"buffer".into()).unwrap_or_default();
    let len = js_sys::Reflect::get(&buf, &"byteLength".into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    len / 1_048_576.0
}

#[cfg(not(target_arch = "wasm32"))]
fn mem_mib() -> f64 {
    0.0
}

/// 把 panic 路由到 JS，避免真机静默白屏。
#[cfg(target_arch = "wasm32")]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(['\n', '\r'], " ");
        let js = format!(
            "try{{var g=typeof GameGlobal!=='undefined'?GameGlobal:globalThis;\
             if(g&&g.__bevy_panic)g.__bevy_panic(\"{msg}\");\
             if(typeof tt!=='undefined'&&tt.showModal)tt.showModal(\
               {{title:'PANIC',content:\"{msg}\".slice(0,600),showCancel:false}});\
             }}catch(e){{}}"
        );
        let _ = js_sys::eval(&js);
    }));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn install_panic_hook() {}

/// 证明 `Update` 在推进，并记录它看到了什么。
///
/// 用 `Local<u32>` 而不是 `Time` 计数：如果渲染循环没跑起来，`Time` 会一直是
/// 0，按时间节流反而会把最需要看的第一帧藏掉。
fn diag_tick(
    mut frame: Local<u32>,
    time: Res<Time>,
    cams: Query<&Camera>,
    sprites: Query<&Sprite>,
    clear: Res<ClearColor>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    *frame += 1;
    let n = *frame;
    if n > 6 && n % 300 != 0 {
        return;
    }
    let (w, h) = windows
        .single()
        .map(|w| (w.physical_width(), w.physical_height()))
        .unwrap_or((0, 0));
    let c = clear.0.to_srgba();
    say(format_args!(
        "diag: update #{n} t={:.2}s cams={} sprites={} clear=({:.2},{:.2},{:.2}) win={}x{} mem={:.1}",
        time.elapsed_secs(),
        cams.iter().count(),
        sprites.iter().count(),
        c.red,
        c.green,
        c.blue,
        w,
        h,
        mem_mib()
    ));
}

/// 在已经装好插件的 app 上追加诊断系统。
pub fn install(app: &mut App) {
    if !enabled() {
        return;
    }
    say(format_args!("probe: on (mem {:.1} MiB)", mem_mib()));
    app.add_systems(Update, diag_tick);
}
