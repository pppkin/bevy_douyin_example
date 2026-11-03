# 适配层契约

本文记录 `addons/bevy-douyin`（tag `adapter-v0.2.1`）对外承诺的行为，以及本示例
依赖了其中哪些部分。改适配层或升级 tag 之后，建议对照本文重新验证。

涉及的文件：

- `addons/bevy-douyin/runtime/adapter.js` —— 所有 `tt.*` 调用的唯一入口
- `addons/bevy-douyin/runtime/bootstrap.js` —— 安装 shim、加载 glue 的启动器
- `addons/bevy-douyin/tools/build.sh` —— 构建与 patch 流程
- `platforms/douyin/package/game.js` —— 本示例的启动参数

---

## 1. 启动时序

```
game.js
  require('./runtime/bootstrap.js')
    runtime.start({...})
      1. adapter.install(cfg)      // 安装 shim，发布宿主全局
      2. require(glue)             // 此时 window / document / GL 已就绪
      3. glue({ module_or_path })  // 加载 pong.wasm.br
      4. wasm 启动，Bevy 运行
```

顺序不能变。`adapter.install()` 会：

1. 用 `tt.createCanvas()` 创建画布，并按屏幕比例计算逻辑尺寸；
2. 在 `globalThis` 上定义 shim `window`（以及 `document`、`performance`、
   `localStorage`、RAF 等）；
3. 把宿主事实发布到 **两个** 地方：`GameGlobal[prefix + name]` 和
   `window[prefix + name]`；
4. 注册 `tt.onTouch*`、`tt.onShow/onHide`、`tt.onThemeChange` 等回调。

`bootstrap.js` 还提供 `env`、`__bevyBoot`、`__bevyBootError`、`onReady`、
`onError`，排错时可以读。

### 为什么 Rust 侧读的是 `window` 而不是 `GameGlobal`

抖音的 wasm 运行时里：

```js
eval("typeof GameGlobal")   // 在 wasm 侧不可用：eval is not a function
```

而且 wasm 的全局对象不是小游戏的 `GameGlobal`。适配层把 shim `window` 装在
`globalThis` 上，所以 Rust 侧这样读：

```rust
fn js_window() -> JsValue {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("window"))
        .unwrap_or(JsValue::UNDEFINED)
}

fn host_global(name: &str) -> JsValue {
    js_sys::Reflect::get(&js_window(), &JsValue::from_str(&format!("{GLOBAL_PREFIX}{name}")))
        .unwrap_or(JsValue::UNDEFINED)
}
```

这个坑很隐蔽：读不到时所有值都会退化成默认值，游戏"能跑"，但画布尺寸、安全区、
存档全部失效。示例因此把默认值设计成 720×1280 的桌面值，并在日志里打印实际读到的
尺寸，见 `platforms/douyin/src/host.rs`。

---

## 2. 全局表

默认前缀 `__bevy_douyin_`（`game.js` 的 `globalPrefix` 必须和
`platforms/douyin/src/host.rs` 的 `GLOBAL_PREFIX` 一致）。

| 全局 | 类型 | 方向 | 说明 |
| --- | --- | --- | --- |
| `Canvas` | `{width, height}` | JS → Rust | 逻辑画布尺寸，单位是画布像素 |
| `SafeArea` | 见下 | JS → Rust | 安全区 inset（CSS px）、屏幕尺寸、胶囊矩形 |
| `Theme` | `"dark" \| "light"` | 双向 | 当前主题 |
| `SetTheme(value)` | `(string) => void` | Rust → JS | 覆盖主题并同步 `Theme` |
| `ShowAd(kind)` | `(string) => boolean` | Rust → JS | 拉起激励视频，返回是否受理 |
| `AdStatus` | `string` | JS → Rust | `idle/loading/ended/skipped/error/unavailable` |

`SafeArea` 的字段：

```ts
{
  left: number, top: number, right: number, bottom: number, // CSS px
  screenWidth: number, screenHeight: number,               // CSS px
  menuButton: { left, top, right, bottom, width, height }  // 右上角胶囊，CSS px
}
```

`menuButton.bottom` 是**胶囊底边**，不是高度。示例用
`max(safe.top, menuButton.bottom)` 作为顶部内缩，否则长屏手机上计分条会压到胶囊。

---

## 3. 画布尺寸：为什么不是简单的 720×1280

适配层接收设计分辨率 `width/height`（`game.js` 里是 720×1280），然后按屏幕比例
推导实际画布，保证不变形、也不留黑边：

```js
W = round(max(designW, designH * screenW / screenH))
H = round(max(designH, designW * screenH / screenW))
```

效果：

| 屏幕 | 画布 |
| --- | --- |
| 竖屏长手机（如 393×852） | 720×1561，宽度保持 720，高度变高 |
| 4:3 平板 | 保持高度 1280，宽度变宽 |

所以**不要**在 Rust 里写死 `720×1280`。本示例在启动时读 `Canvas`：

```rust
let (canvas_w, canvas_h) = host.canvas();
WindowResolution::new(canvas_w, canvas_h)
```

相机再把世界宽度固定为 720，高度跟随画布比例：

```rust
Projection::Orthographic(OrthographicProjection {
    scaling_mode: ScalingMode::Fixed { width: 720.0, height: view.height },
    ..OrthographicProjection::default_2d()
})
```

这样世界坐标里 `1 单位 == 1 画布像素`，画布永远是 720 宽，只是高矮不同。所有布局
都从 `View { width: 720, height }` 推导，天然适配长短屏。

> 适配层把 `devicePixelRatio` 固定为 1（或配置值），所以在本管线里
> `窗口逻辑尺寸 == 画布像素尺寸`。模拟器实测：`phys=720×1561 logical=720×1561
> scale=1`，正交投影 `area = -360..360 × -780.5..780.5`。

---

## 4. 三种像素单位

这是最容易搞混的地方。示例里严格区分：

| 单位 | 来源 | 用途 |
| --- | --- | --- |
| **CSS px** | `tt.getSystemInfoSync()`、`SafeArea` | 屏幕/胶囊尺寸 |
| **画布 px** | `Canvas`、`PointerEvent.offsetX/Y` | 指针位置、窗口尺寸 |
| **世界单位** | 相机坐标系，中心原点、y 向上 | 所有游戏逻辑 |

换算（见 `platforms/douyin/src/host.rs`）：

```rust
px_to_world  = 720 / canvas_w;          // 画布 px → 世界
css_to_world = 720 / safe.screen_width; // CSS px → 世界

// 指针：左上角原点 → 中心原点、y 向上
world.x = (canvas_x - canvas_w / 2) * px_to_world;
world.y = (canvas_h / 2 - canvas_y) * px_to_world;
```

在 393×852 的模拟器上，`canvas = 720×1561`、`screenWidth = 393`，因此
`css_to_world ≈ 1.832`、`px_to_world = 1`。一个 CSS 像素约等于 1.832 个世界单位。

---

## 5. 输入：触摸如何变成 `cursor_position()`

适配层监听 `tt.onTouchStart/Move/End/Cancel`，合成 `PointerEvent`：

```js
{
  clientX: x,
  offsetX: x * (canvasW / screenW),   // 画布像素
  offsetY: y * (canvasH / screenH),
  pointerType: 'touch',
  button: type === 'pointermove' ? -1 : 0,
}
```

winit 0.30 的 `mouse_position` 读的是 `offsetX/offsetY`，并且用
`devicePixelRatio`（这里为 1）作为 scale factor。结果：

- `window.cursor_position()` 直接就是**画布像素**；
- `Touch::position()` 也是画布像素；
- 两者都只需要 `View::pointer()` 一次换算。

触摸还会产生一次合成鼠标事件（click），所以示例在 `handle_taps` 里做了 250ms /
24 世界的去重：

```rust
if let Some((then, at)) = *last_tap {
    if now - then < 0.25 && (tap - at).length() < 24.0 {
        continue;
    }
}
```

没有这层去重时，点一次主题按钮会触发两次切换（`dark → light → dark`），
日志里能看到两条相邻帧的 `pong: theme ->`。

---

## 6. 主题

- 启动时读取 `Theme`；
- 适配层每 **4000ms** 轮询 `tt.getSystemInfoSync().theme`，也监听
  `tt.onThemeChange`；
- Rust 调 `SetTheme("light" | "dark")` 可覆盖，并立即更新 `Theme` 全局；
- 示例把用户选择写进 `localStorage` 的 `pong.theme`，下次启动优先恢复；
  `sync_environment` 每 30 帧（约 0.5 秒）检查一次 `Theme` 是否被系统改掉，
  **手动切换过之后（`Look.custom = true`）就不再跟随系统**。

`paint_scene` 每帧根据 `Look.theme` 取调色板，并同步 `ClearColor`，所以切换是
立即生效的，不需要重建任何实体。

---

## 7. 激励视频

`ShowAd(kind)` 是**同步返回 boolean** 的：

| 情况 | 返回 | `AdStatus` |
| --- | --- | --- |
| 没有 `adUnitId` | `false` | `unavailable` |
| 有广告位，拉起成功 | `true` | `loading` → `ended` / `skipped` |
| 拉起失败 | `true`（内部会重试 load） | `error` |

`onClose` 会发布 `ended`（看完）或 `skipped`（中途关闭）；`onError` 发布 `error`。

适配层**没有内置 mock**。`template/package/game.js` 的注释说"留空走 mock"，实际
需要游戏自己实现。本示例的策略（`Ads` 资源）：

```
点击胶囊
  ├─ ShowAd() == true  → AdPhase::Pending，轮询 AdStatus
  │                        ended      → 给奖励
  │                        skipped    → 取消
  │                        error      → 取消
  │                        超时 30s   → 取消
  └─ ShowAd() == false → AdPhase::Mock(0.9s) → 给奖励
```

奖励是 8 秒慢动作：`Time<Virtual>::set_relative_speed(0.45)`，用 `Time<Real>`
计时，避免慢动作把自己的倒计时也拖慢。配置真实广告位只需要改 `game.js` 的
`AD_UNIT_ID`，Rust 侧无需改动。

---

## 8. 存储

`window.localStorage` / `sessionStorage` 是同一个对象，底层落到
`tt.setStorageSync/getStorageSync`，支持 `getItem/setItem/removeItem/clear`。

示例的键：

| key | 内容 |
| --- | --- |
| `pong.best_rally` | 最高连续对拉 |
| `pong.wins.ai` | AI 胜场 |
| `pong.wins.player` | 玩家胜场 |
| `pong.theme` | `dark` / `light` |

写入是同步的，分数变化时直接 `setItem` 即可，量很小，不需要节流。真机冷启动后
读回验证过一次：日志会打印 `pong: best rally N`。

---

## 9. 适配层没有提供 / 不能依赖的东西

- **`eval`**：抖音 wasm 运行时里不可用，抛 `TypeError: eval is not a function`。
- **`GameGlobal`**：wasm 侧看不到；它只在小游戏 JS 主环境里。
- **真正的 DOM**：`document` 是 shim，只覆盖 winit/bevy 用到的少量接口。
- **多 canvas / 离屏 canvas**：适配层只创建一个游戏画布。
- **音频**：本示例没启用 `bevy_audio`；需要音频时要确认适配层对 `AudioContext`
  的 shim 情况。
- **横屏**：`game.json` 固定竖屏，适配层的尺寸推导也按竖屏设计。

---

## 10. 升级适配层时重点回归

1. `Canvas` / `SafeArea` 字段是否变化（尤其 `menuButton`）；
2. `ShowAd` 的返回值语义和 `AdStatus` 枚举；
3. 指针是否仍然是 `offsetX/offsetY` + `devicePixelRatio = 1`；
4. `patch_glue.py` 是否仍匹配 wasm-bindgen 0.2.129 的输出；
5. `tools/build.sh` 的 wasm-opt 参数是否仍排除 `--all-features`。

升级后至少跑一遍 [device-checklist.md](device-checklist.md) 的模拟器部分。
