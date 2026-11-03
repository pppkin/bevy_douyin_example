# 代码导览

这份文档解释 `platforms/douyin/src/` 里的每个模块、系统顺序和可调参数。读完之后
应该能独立改动玩法，而不破坏平台适配层。

```
platforms/douyin/src/
├── main.rs   启动：窗口、插件、渲染容错、相机
├── host.rs   平台桥：Canvas / SafeArea / Theme / ShowAd / localStorage
├── game.rs   玩法：布局、输入、物理、计分、调色板、广告状态机
└── probe.rs  诊断：probe 开关、panic 弹窗、内存日志
```

---

## 1. 启动链路（main.rs）

```rust
probe::install_panic_hook();      // 真机 panic 弹窗，必须最先装

let host = Host::new();           // 读 __bevy_douyin_* 全局
let view = host.view();           // 换算世界坐标布局
let (canvas_w, canvas_h) = host.canvas();

App::new()
    .insert_resource(ClearColor(Color::BLACK))
    .insert_resource(host)
    .insert_resource(view)
    .insert_resource(WinitSettings::continuous())   // ① 真机不冻结
    .add_plugins(DefaultPlugins.set(WindowPlugin { ... }))
    .insert_resource(RenderErrorHandler(|error, ..| { // ② 错误不停渲染
        error!("RenderError {:?}: {}", error.ty, error.description);
        RenderErrorPolicy::Ignore
    }))
    .add_systems(Startup, force_opaque_window);      // ③ 不透明合成
```

三个设备相关的硬性要求，缺一个就会出现"第一帧后黑屏 / 跑几帧停住 / 手机全黑"：

| 代码 | 为什么 |
| --- | --- |
| `WinitSettings::continuous()` | 小游戏页面收不到 focus 事件，默认 reactive 模式几帧后停止更新 |
| `RenderErrorHandler { Ignore }` | WebGL2 上任何一次 wgpu 错误默认会 `StopRendering` |
| `CompositeAlphaMode::Opaque` | 画布按预乘 alpha 合成，帧缓冲 alpha=0 时手机看到黑色 |

`Window` 的关键字段：

```rust
Window {
    canvas: Some("#pong-canvas".into()),          // 与 game.js 的 canvas 一致
    resolution: WindowResolution::new(canvas_w, canvas_h), // 来自适配层，别写死
    resizable: false,
    ..default()
}
```

`Msaa::Off` 是在 `game::setup` 里、相机 spawn 之后用 `commands.entity(camera).insert(Msaa::Off)`
插入的。**不能**在 spawn 元组里再写一个 `Msaa`：`Msaa` 是 `Camera2d` 的必需组件，
重复插入会 panic。WebGL2 也建不出 MSAA 管线。

---

## 2. 平台桥（host.rs）

`Host` 在启动时把宿主信息读成普通 Rust 值，之后不再持有 `JsValue`，所以可以安全
地作为 Bevy `Resource` 跨线程使用。

```rust
pub struct Host { canvas_w: f32, canvas_h: f32, safe: SafeArea }
pub struct View {
    width: f32, height: f32,
    inset_left/top/right/bottom: f32,
    canvas_w/canvas_h: f32,
    px_to_world: f32,
}
```

### 布局推导

```rust
width  = 720
height = canvas_h * 720 / canvas_w
inset_* = safe.* * 720 / safe.screen_width
inset_top = max(safe.top, safe.menuButton.bottom) * 720 / safe.screen_width
```

模拟器实测：画布 720×1561，安全区 `top=0`、`menuButton.bottom≈101 CSS`、
`bottom≈34 CSS`：

```
世界高度        = 1561
inset_top       = 101 * 1.832 ≈ 185
inset_bottom    = 34  * 1.832 ≈ 62
```

`SafeArea` 是 CSS px，`Canvas` 是画布 px，两者不能混用。`View` 在构造时就完成了
换算，业务代码只接触世界单位。

### 指针换算

```rust
pub fn pointer(&self, canvas_pos: Vec2) -> Vec2 {
    Vec2::new(
        (canvas_pos.x - self.canvas_w * 0.5) * self.px_to_world,
        (self.canvas_h * 0.5 - canvas_pos.y) * self.px_to_world,
    )
}
```

`window.cursor_position()` 和 `Touch::position()` 都已经是画布像素，直接传进来即可。

### 其它 API

| 方法 | 说明 |
| --- | --- |
| `canvas() -> (u32, u32)` | 画布尺寸，用于创建窗口 |
| `view() -> View` | 布局 |
| `theme() -> Option<Theme>` | 读 `Theme` 全局 |
| `set_theme(Theme)` | 调 `SetTheme`，覆盖并同步 |
| `show_ad() -> bool` | 调 `ShowAd("rewarded")`，false 表示无广告位 |
| `ad_status() -> Option<String>` | 读 `AdStatus` |
| `load_u32 / save_u32 / load_str / save_str` | 走 shim `localStorage` |

非 wasm 构建（`cargo check`、IDE）返回 720×1280、无安全区、无存储，所有平台调用
走 `cfg` 分支，不引入 `wasm-bindgen` 依赖。

---

## 3. 玩法（game.rs）

### 3.1 资源

| 资源 | 作用 |
| --- | --- |
| `Field` | 可玩矩形：`left/right/top/bottom`，已避开安全区和顶/底 UI |
| `View` | 来自 `host.rs`，只读 |
| `Match` | 当前局比分、总胜场、连续对拉、最高纪录、阶段、发球方向、闪光 |
| `Phase` | `Serve(timer)` / `Rally` / `Over { timer, winner }` |
| `Rng` | 32 位 LCG，发球角度用；不引入 `rand` |
| `Pointer` | 最近一次指针位置与 idle 时间 |
| `Effects` | 慢动作剩余秒数 |
| `Ads` | 广告状态机：`Idle / Pending / Mock / Error` |
| `Look` | 当前主题、是否用户手动覆盖 |
| `Themed`（组件） | 精灵的语义角色，`paint_scene` 据此上色 |
| `Role` | `Frame/Line/Player/Ai/Ball/PipOff/Chip/ChipIcon/Accent/Record` |

### 3.2 系统顺序

```rust
(update_player, update_ai, step_ball, handle_taps,
 sync_environment, tick_effects, paint_scene).chain()
```

| 系统 | 职责 |
| --- | --- |
| `update_player` | 指针/触摸/键盘 → 右拍目标 y，限速 3200/s，夹在场地内 |
| `update_ai` | 预测落点 + 摆动 → 左拍，速度随比分/回合提升，上限 980/s |
| `step_ball` | 发球、子步进物理、挡板反弹、计分、拖尾、胜负 |
| `handle_taps` | 主题按钮、广告胶囊；触摸/鼠标去重 |
| `sync_environment` | 每 30 帧跟随系统主题（手动切换后停止） |
| `tick_effects` | 慢动作计时、广告状态机推进、闪光衰减 |
| `paint_scene` | 按主题上色、更新计分块/进度条/胜者边框 |

顺序有意义：先移动，再碰撞，再处理点击，最后统一上色。`paint_scene` 放最后，
保证当帧所有状态变化都能反映到颜色上。

### 3.3 场地布局

```rust
left   = -W/2 + inset_left  + 28
right  =  W/2 - inset_right - 28
top    =  H/2 - inset_top   - 86
bottom = -H/2 + inset_bottom + 118
```

- 左右 28：边框留白；
- 顶部 86：计分条 + 主题按钮；
- 底部 118：广告胶囊 + 进度条。

球拍 x：`left + 26` / `right - 26`；广告胶囊 y：`bottom - 52`。

### 3.4 物理与节奏

| 参数 | 值 |
| --- | --- |
| 胜利分数 | 7 |
| 球拍 | 18 × 132 |
| 球 | 22 × 22 |
| 拖尾 | 5 段，透明度 0.30 → 0.08 |
| 发球速度 | `540 + 总比分 * 18`，垂直分量随机 ±260 |
| 每次击球加速 | `× 1.05`，上限 1180 |
| 反弹角度 | 击球点偏移（-1..1）× 0.85 rad |
| 子步进 | `ceil(speed * dt / 5)`，夹在 1..16，防穿透 |
| 得分判定 | 球心越过 `left - 40` / `right + 40` |
| 得分后发球 | 0.85s；一局结束后 1.0s |
| `Over` 展示 | 2.8s |
| 慢动作 | 8s，`Time<Virtual>` 速度 0.45 |

`dt` 在物理/输入里都夹了上限（0.033 / 0.05），避免切后台回来一帧穿透。

### 3.5 输入与点击区

右拍同时接受：

1. `window.cursor_position()`（鼠标 / 触摸合成的 pointer）；
2. `Touches::iter().next()`（真触摸）；
3. `↑/↓`、`W/S`（桌面/模拟器调试）。

触摸与合成鼠标可能落在相邻两帧，所以 `handle_taps` 用
`Local<Option<(f32, Vec2)>>` 做 250ms / 24 世界单位的去重。

点击热区：

| 目标 | 中心 | 半尺寸 |
| --- | --- | --- |
| 主题按钮 | `(field.left + 24, field.top + 36)` | 30 × 30 |
| 广告胶囊 | `(0, field.chip_y())` | 110 × 36 |

热区比视觉尺寸大，触摸更宽容；两个热区不重叠。

### 3.6 计分与存档

- 7 个计分块/方：左=AI、右=玩家，赢一分点亮一个；
- 顶部中央菱形：追平或刷新最高连续对拉时点亮；
- 局末 `Over` 阶段边框染成胜者颜色，球隐藏；
- 赢一局后胜场持久化，然后比分归零。

存档键：`pong.best_rally`、`pong.wins.ai`、`pong.wins.player`、`pong.theme`。
写入点只有三处：刷新最高连击、赢一局、切换主题。

### 3.7 主题与调色板

`Palette::of(Theme)` 给出 11 个颜色：`bg/frame/line/player/ai/ball/pip_off/
chip/chip_icon/accent/warn`。`paint_scene` 同时设置 `ClearColor`，所以背景、
边框、文字替代物（计分块）全部一起切换。

`Look.custom`：用户点过主题按钮后为 true，`sync_environment` 不再跟随系统。

### 3.8 广告状态机

```
Idle ──点击──> ShowAd() true ──> Pending ──AdStatus ended──> 奖励
                    │                     ├─ skipped/error ─> Error(1.2s)
                    │                     └─ 30s 超时 ───────> Error(1.2s)
                    └─ false ──> Mock(0.9s) ──> 奖励
Error(t) ──t 归零──> Idle
```

奖励 = `fx.slowmo = 8.0`，`tick_effects` 把 `Time<Virtual>` 的
`relative_speed` 设为 0.45。倒计时用 `Time<Real>`，否则慢动作会把自己的 8 秒
也拉长。

`Pending` 期间再点胶囊不会重复请求（`matches!(ads.phase, AdPhase::Idle)` 且
`fx.slowmo <= 0` 才受理）。

---

## 4. 诊断（probe.rs）

默认关闭，零开销（一个 atomic 判断）。

开启方式（真机/模拟器控制台）：

```js
GameGlobal.__probe = 1;
```

开启后：

- 前 6 帧每帧打印 `diag: update #n t=.. cams=.. sprites=.. clear=.. win=WxH mem=..`；
- 之后每 300 帧一次；
- `mem_mib()` 通过 `wasm_bindgen::memory()` 读 wasm 线性内存大小。

panic 钩子始终生效：把 `file:line` 和消息通过 `tt.showModal({title:'PANIC'})`
弹出。真机 `panic = "abort"` + 控制台被吞时，这是唯一能看到 panic 的方式。

---

## 5. 想改玩法时

| 目标 | 改动 |
| --- | --- |
| 更快/更慢 | `game.rs` 顶部的发球速度、加速系数、AI 速度 |
| 改成 11 分制 | `WIN_SCORE`，计分块数量（`setup` 里的循环） |
| 加音效 | `Cargo.toml` 加 `bevy_audio`，在碰撞/得分处触发；先确认适配层音频能力 |
| 加文字 | 需要字体资源与 `bevy_text`，并处理资源拷贝（适配层会把 `assets/` 拷进包） |
| 横屏 | `game.json` 改 `landscape`，同时调整 `game.js` 的 `width/height` 与布局 |
| 换配色 | `Palette::of` 两组颜色 |
| 真广告 | `package/game.js` 的 `AD_UNIT_ID` |

改完先 `cargo check --target wasm32-unknown-unknown`，再 `./build.sh`，最后在
开发者工具里重新编译（改了 wasm 必须重新导入/编译，热重载只对 JS 有效）。
