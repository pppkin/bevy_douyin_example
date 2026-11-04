# 排错手册

按"症状"查。每条都给出**原因**和**在本仓库里的具体处理**。示例本身已经踩过这些坑，
所以大部分问题在代码里已经有了对应修复，这里解释为什么。

---

## 快速定位流程

1. **开发者工具控制台**有没有红字？先看第一条错误，后面的多半是连锁反应。
2. 页面白屏/黑屏时，在控制台执行：

   ```js
   __bevyBootError        // bootstrap 捕获的启动错误
   __bevyBoot             // 启动状态
   typeof GameGlobal
   typeof window
   ```

3. 打开 probe 再重进：

   ```js
   GameGlobal.__probe = 1;
   ```

4. 仍然没有日志 → 大概率是 panic 被 `panic = "abort"` 吞了，检查 `PANIC` 弹窗
   是否出现；没有弹窗说明 panic hook 没装上或 wasm 根本没实例化。
5. 区分"模拟器问题"和"真机问题"：同一份包在模拟器正常、真机异常，优先看第 6 节。

---

## 1. 启动 / 黑屏

### 1.1 第一帧后黑屏

**症状**：画面闪一下（或显示一帧 UI）后全黑，控制台有
`RenderError ...` 或 wgpu validation 错误。

**原因**：Bevy 默认在渲染错误时 `StopRendering`。WebGL2 上某些管线/格式不被支持，
第一次错误之后就不再出帧。

**处理**：`platforms/douyin/src/main.rs` 已经注册：

```rust
.insert_resource(RenderErrorHandler(|error, _main, _render| {
    error!("RenderError {:?}: {}", error.ty, error.description);
    RenderErrorPolicy::Ignore
}))
```

如果你把它删了，这个症状会回来。注意 `Ignore` 只是"继续渲染"，错误本身仍需修复；
看日志里的 `error.ty` / `description`。

### 1.2 真机全黑，模拟器正常

**原因**：WebGL 画布按预乘 alpha 合成，帧缓冲 alpha 为 0 时手机显示黑色。

**处理**：

```rust
window.composite_alpha_mode = CompositeAlphaMode::Opaque;
```

已放在 `Startup` 的 `force_opaque_window` 里。

### 1.3 跑几帧后停住（不黑，但画面不动）

**原因**：小游戏页面不会给 winit 发送 focus 事件，默认 `WinitSettings::reactive`
在失去焦点后停止更新。

**处理**：

```rust
.insert_resource(WinitSettings::continuous())
```

### 1.4 MSAA 相关 panic

**症状**：启动即 panic，堆栈里有 `Msaa` 或 "already contains component"。

**原因**：`Msaa` 是 `Camera2d` 的必需组件。在 spawn 元组里再写一个 `Msaa::Off`
会重复插入；而 WebGL2 也建不出 MSAA 管线。

**处理**：先 spawn 相机，再：

```rust
commands.entity(camera).insert(Msaa::Off);
```

### 1.5 白屏 / `require` 失败

**症状**：控制台 `module ... is not defined` / `Cannot find module './pong.js'`。

**原因**：

- 没有先构建就导入项目；
- 导入的目录不对（导入了仓库根目录）；
- `game.js` 里的 `wasm` / `glue` 文件名和产物不一致。

**处理**：确认 `platforms/douyin/package/` 下有 `pong.js`、`pong.wasm.br`、
`runtime/`，并且导入的就是这个目录。

---

## 2. wasm / 构建期错误

### 2.1 `error[B0001]: ... accesses component(s) ... in a way that conflicts`

完整形式：

```
panicked at bevy_ecs-0.20.1/src/query/state.rs:216:13:
error[B0001]: <Enable the debug feature to see the name> in system ...
accesses component(s) ... in a way that conflicts with a previous system parameter ...
```

**原因**：同一个系统里两个 Query 对同一组件的访问（一个 `&mut`、一个 `&`）在
archetype 上可能重叠。Bevy 运行时会直接 panic，在抖音里表现为
`RuntimeError: unreachable`，且只有 `raf callback #1` 一行，非常难查。

**本仓库的真实案例**（`step_ball`）：

```rust
mut trail_query: Query<(&Trail, &mut Transform), Without<Ball>>,
paddles: Query<(&Paddle, &Transform), Without<Ball>>,
```

`With<Trail>` 和 `With<Paddle>` 并不互斥，两个 Query 都可能匹配同一个实体。

**处理**：让过滤器互斥：

```rust
mut trail_query: Query<(&Trail, &mut Transform), (Without<Ball>, Without<Paddle>)>,
paddles: Query<(&Paddle, &Transform), (Without<Ball>, Without<Trail>)>,
```

经验：只要两个参数碰同一个组件，就检查它们的过滤器组合；`Without<X>` 两边都写
不一定够，必须能证明"不可能同时匹配"。

### 2.2 `unknown import kind 0x7f`

**原因**：`wasm-opt` 用了 `--all-features`，binaryen 133 会生成抖音不支持的精简
导入（compact imports，kind 0x7f）。

**处理**：适配层的 `tools/build.sh` 已经改成显式特性列表：

```
--enable-bulk-memory --enable-mutable-globals --enable-sign-ext
--enable-nontrapping-float-to-int --enable-reference-types --enable-multivalue
```

不要加 `--all-features`。

### 2.3 `patch_glue.py` 报找不到字符串

**原因**：`wasm-bindgen` crate 与 CLI 版本不一致，生成的 glue 结构和补丁脚本的
匹配规则对不上。

**处理**：

```bash
wasm-bindgen --version        # 0.2.129
grep wasm-bindgen platforms/douyin/Cargo.toml
```

两边都锁到 `0.2.129`，然后 `cargo clean -p pong_douyin` 或删 `target/` 重编。

### 2.4 构建成功但页面报语法错误（`catch {`）

**原因**：`esbuild --target=es2020` 会保留可选 catch 绑定，抖音预览编译器不认。

**处理**：适配层用 `--target=es2017`，并在结尾检查产物里没有 `catch{`。不要改成
es2020+。

### 2.5 `wasm-bindgen` 版本警告

构建脚本会警告 but 继续。警告本身不致命，但如果后面出现 2.3 的补丁失败，优先
按版本不一致处理。

---

## 3. 宿主数据读不到

### 3.1 画布永远是 720×1280、没有安全区、存档不生效

**原因**：Rust 侧读不到适配层发布的全局，所有值走了默认分支。

最常见的两个原因：

1. **用了 `js_sys::eval`**。抖音 wasm 运行时里没有 `eval`：

   ```
   JsValue(TypeError: eval is not a function ... at __wbg_eval ...)
   ```

2. **用了 `GameGlobal`**。wasm 的全局对象不是小游戏的 `GameGlobal`。

**处理**：先取 shim `window`，再读带前缀的属性：

```rust
fn js_window() -> JsValue {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("window"))
        .unwrap_or(JsValue::UNDEFINED)
}

let key = format!("__bevy_douyin_{name}");
js_sys::Reflect::get(&js_window(), &JsValue::from_str(&key))
```

验证：日志应打印真实画布尺寸，例如 `pong: canvas 720x1561 -> world 720x1561`。

### 3.2 顶部计分条压到胶囊

**原因**：只用了 `SafeArea.top`。很多机型 `top` 是 0，但右上角有胶囊按钮。

**处理**：

```rust
let top_css = safe.top.max(safe.menu_button_bottom);
```

然后换算到世界坐标。`menuButton.bottom` 是胶囊**底边**（CSS px），不是高度。

### 3.3 主题不跟随系统 / 切换后又被改回去

- 只在手动切换前跟随系统；点过主题按钮后 `Look.custom = true`，停止跟随，这是
  预期行为。0.3 起适配层也会保持手动值（`ThemeManual = true`），系统切换不会把
  `Theme` 改回去；要恢复跟随，Rust 侧调用 `FollowTheme()`（示例保留了这个 API，
  但没有做"恢复跟随"按钮）。
- 适配层每 4 秒轮询一次系统主题，系统切换后最多约 4 秒生效；游戏侧再每 30 帧
  读一次 `Theme`。如果一直不生效，先确认 `ThemeManual` 是否为 `false`，再确认
  `tt.onThemeChange` 是否被宿主触发。

---

## 4. 输入

### 4.1 点一次按钮触发两次（主题闪两下）

**原因**：触摸产生 touch 事件 + 合成鼠标事件，二者可能落在相邻两帧。

**处理**：`handle_taps` 对同一点位做 250ms / 24 世界单位去重。去掉这段代码就会
复现。日志特征：

```
pong: theme -> light
pong: theme -> dark
```

同一帧出现两条。

### 4.2 点击位置和视觉对不上

**原因**：把 CSS px 当成了画布 px，或忘了 y 轴翻转。

**检查**：

- `SafeArea` 是 CSS px，`Canvas` 是画布 px；
- 指针是画布 px，左上角原点；
- 世界坐标是中心原点、y 向上；
- 相机 `ScalingMode::Fixed { width: 720, height: view.height }`，世界 1 单位 =
  1 画布像素。

调试时打印一次：

```rust
info!("tap={:?} theme={:?} chip={:?}", tap, theme_center, chip_center);
```

如果 `tap` 与预期中心差一个固定比例，多半是 `screenWidth` / `canvas_w` 用混了。

### 4.3 真机拖动球拍跳变 / 不跟手

- 确认没有同时把 mouse 和 touch 都当作新目标（示例里 touch 优先）；
- 检查 `dt` 是否夹了上限（切后台回来会有一帧超大 dt）；
- 球拍移动速度上限是 3200/s，快速甩动时会"追不上"，这是刻意的手感限制。

---

## 5. 广告

### 5.1 点胶囊没有任何反应

按顺序排查：

1. `AD_UNIT_ID` 是否为空？空且没有关 mock → 适配层应打印
   `ad mock kind=rewarded result=ended delay=600ms ticket=1`，游戏打印
   `pong: rewarded ad requested ticket=1`，约 0.6s 后
   `ad result ticket=1 status=ended reason=mock` 和 `ad mock finished: ended`，
   随后 `pong: rewarded ad ended ticket=1 mock=true reason=mock, slow-mo granted`。
2. 只有看到 `pong: ad request rejected (...)` 才是游戏自己的 0.9s 兜底，说明
   `RequestAd` 返回了 0（通常是 `mockAds: false`、已有请求在处理，或广告位创建
   失败）；括号里是 `AdStatus` / `AdLastError`。
3. 控制台完全没有广告日志 → 点击没命中热区（胶囊中心 `(0, bottom-52)`，
   半尺寸 110×36）。
4. 有广告位但没拉起 → 看 `AdReason`：`create-failed` 表示
   `tt.createRewardedVideoAd` 不可用，`show-failed` / `retry-failed` 表示拉起
   失败，`timeout` 表示 30s 内没有终态。
5. `Pending` 状态下再点不会重复请求（`RequestAd` 返回 0），等 `AdResult` 或
   30 秒超时。

### 5.2 看完广告没给奖励

看 `AdResult`（先用 `ticket` 确认是不是当前这一单）：

| `status` / `reason` | 结果 |
| --- | --- |
| `ended`（`isEnded=true`） | 给奖励 |
| `skipped` | 不给（`Error` 提示 1.2s） |
| `error` / `unavailable` | 不给 |
| `error` + `reason=timeout` | 30 秒无终态，不给 |

真机上 `ended` 只在 `onClose({isEnded: true})` 时发布；有些基础库版本字段不同，
升级适配层时要回归。若日志里的 ticket 与游戏记录的不一致，说明是上一单的迟到
回调，游戏会忽略。

### 5.3 模拟器永远走 mock

适配层 0.2.2 起，没有 `adUnitId` 时由适配层直接模拟，模拟器上这是预期。
要验证真实流程必须用真机 + 真实 `adUnitId`；也可以设置 `mockAdResult: 'skipped'`
或 `'error'` 验证中途关闭/失败分支，设置 `mockAdDelay: 5000` 验证 Pending 期间
重复点击不会重复请求。

### 5.4 适配层 mock 和游戏兜底重复给奖励

不会。适配层 mock 会让 `RequestAd` 返回非 0 ticket，游戏进入 `Pending` 等待
`AdResult`，不会再走 `AdPhase::Mock`。只有 `RequestAd` 返回 0 时才走 0.9s 本地
兜底；两者互斥。

---

## 5.5 音频

先确认启动日志：

```text
[bevy-douyin:pong] audio: context ready sampleRate=44100 state=running
```

**没有这行**：适配层低于 0.4，或 `tt.getAudioContext` 不可用。模拟器会回退到
原生 `AudioContext`；真机需要基础库 1.34.0+。

**`No audio device found`**：cpal 没拿到上下文。检查是否升级到了 0.4，以及是否
在 `adapter.install()` 之前初始化了 Bevy。

**有上下文但没声音**：

- `state=suspended`：iOS 需要用户手势。点一下屏幕，适配层会在 `touchstart`
  时 `resume()`；
- 系统静音键、音量、蓝牙耳机；
- 确认 `Cargo.toml` 里有 `bevy_audio` + `wav`（或实际使用的格式）；
- WAV 必须是常见 PCM 头；`include_bytes!` 路径从源文件算起；
- 切后台会 `suspend()`，回前台应自动 `resume()`。

**换音效后没声**：`./build.sh` 重新构建，wasm 里的字节是编译期嵌入的，改文件
不会热更新。

---

## 6. 真机 vs 模拟器

| 现象 | 可能原因 |
| --- | --- |
| 模拟器正常，真机黑屏 | 透明合成（`CompositeAlphaMode`）、GPU 能力差异 |
| 模拟器正常，真机字体/布局偏移 | 安全区、胶囊、`screenWidth` 不同 |
| 模拟器能跑，真机 panic 无输出 | `panic = "abort"` + 控制台被吞；靠 `PANIC` 弹窗 |
| 模拟器 60fps，真机卡 | 初始内存不足、过度绘制、wasm 体积；开 probe 看帧日志 |
| 模拟器主题不跟随 | 工具菜单里手动切换系统主题；host 可能不触发 `onThemeChange` |
| 真机广告拉不起 | AppID/广告位不匹配、未过审、频次限制 |

真机日志建议：

```js
GameGlobal.__probe = 1;
// 或者把关键日志写进 storage，事后读出来
tt.setStorageSync('log', lines.join('\n'));
```

---

## 7. 坐标/相机诊断

如果怀疑窗口尺寸、缩放因子或投影不对，临时加一个只跑一次的系统：

```rust
fn diag_layout(
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &Projection), With<Camera2d>>,
) {
    for w in &windows {
        info!("win phys={:?} logical={:?} scale={}",
            w.physical_size(), w.resolution.size(), w.resolution.scale_factor());
    }
    for (c, p) in &cams {
        if let Projection::Orthographic(o) = p {
            info!("ortho area={:?} scale={} viewport={:?}",
                o.area, o.scale, c.logical_viewport_size());
        }
    }
}
```

正常值（模拟器 720×1561）：

```
win phys=UVec2(720, 1561) logical=Vec2(720.0, 1561.0) scale=1
ortho area=Rect { min: Vec2(-360.0, -780.5), max: Vec2(360.0, 780.5) } scale=1
viewport=Some(Vec2(720.0, 1561.0))
```

如果 `scale` 不是 1，说明适配层的 `devicePixelRatio` 没生效，指针换算会整体偏移。

---

## 8. 清理与重来

构建产物、缓存、旧 glue 混在一起时，按顺序清理：

```bash
# 只清 Rust
cargo clean --manifest-path platforms/douyin/Cargo.toml

# 清构建产物（保留手写的 game.js/json/project.config.json）
rm -rf platforms/douyin/package/{pong.js,pong.wasm.br,runtime,assets}

# 完全重来
rm -rf target platforms/douyin/target
git submodule update --init --recursive
./build.sh
```

开发者工具里再执行"清缓存并重新编译"。**不要**手工编辑 `pong.js`：它每次构建都会
被覆盖，而且 `patch_glue.py` 的补丁是幂等性未知的字符串替换。

---

## 9. 仍然卡住时收集什么

- 完整构建日志（从 `./build.sh` 第一行到失败处）；
- 开发者工具控制台第一条错误 + 前后 20 行；
- `wasm-bindgen --version`、`wasm-opt --version`、`rustc --version`；
- `git submodule status`；
- 设备型号、系统版本、抖音版本、基础库版本；
- 是否模拟器/真机、是否配置了真实广告位；
- `GameGlobal.__probe = 1` 后的 `diag:` 日志。
