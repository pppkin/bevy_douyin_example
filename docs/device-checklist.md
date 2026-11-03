# 设备检查清单

从一台干净的机器开始，到模拟器跑通，再到真机。每一步都给出**预期结果**，出现偏差
就去 [troubleshooting.md](troubleshooting.md) 查对应的症状。

---

## 0. 前置：克隆与 submodule

```bash
git clone --recurse-submodules <本仓库地址>
cd bevy_douyin_example

git submodule status
#  a280be628c5d7c2bca2cf5ccf4e50255a905c211 addons/bevy-douyin (adapter-v0.2.1)
```

预期：版本号前**没有** `-` 或 `+`。`-` 表示没初始化，`+` 表示 submodule 的
HEAD 和父仓库记录不一致。

修复：

```bash
git submodule update --init --recursive
# 如果之前手动 checkout 过别的分支：
git -C addons/bevy-douyin checkout adapter-v0.2.1
git add addons/bevy-douyin
```

确认适配层文件存在：

```bash
ls addons/bevy-douyin/runtime/adapter.js \
   addons/bevy-douyin/runtime/bootstrap.js \
   addons/bevy-douyin/tools/build.sh \
   addons/bevy-douyin/tools/patch_glue.py
```

---

## 1. 工具链

```bash
rustc --version          # 1.8x+ 即可，本仓库用 stable
cargo --version
rustup target list --installed | grep wasm32-unknown-unknown

wasm-bindgen --version   # 必须是 0.2.129
wasm-opt --version       # binaryen，实测 133
brotli --version
python3 --version
esbuild --version        # 可选
```

一次性补齐：

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
brew install binaryen brotli
brew install esbuild     # 可选
```

**版本红线**：

- `wasm-bindgen` crate（`platforms/douyin/Cargo.toml`）和 CLI 必须都是 `0.2.129`；
- 否则 `patch_glue.py` 找不到预期的字符串，构建会直接失败（这是好事，不会产出
  一个跑不起来的包）。

---

## 2. 构建

```bash
./build.sh
```

首次约 3 分钟（Bevy 全量编译），之后增量 1~2 分钟。预期结尾能看到：

```
-rw-r--r--  ... 63K  platforms/douyin/package/pong.js
-rw-r--r--  ... 2.3M  platforms/douyin/package/pong.wasm.br
-rw-r--r--  ... 24K  platforms/douyin/package/runtime/adapter.js
-rw-r--r--  ... 2.0K platforms/douyin/package/runtime/bootstrap.js
```

> 体积会随 Bevy 特性、代码量浮动。主包限制是 4MB（Brotli 后），当前约 2.4MB，
> 还有余量。加资源/音频前先算一下。

检查产物齐全：

```bash
ls platforms/douyin/package/
# game.js game.json project.config.json pong.js pong.wasm.br runtime/
```

如果 `runtime/` 或 `pong.js` 缺失，说明构建在拷贝/压缩阶段失败，回看构建日志，
不要拿旧产物凑合（旧 `pong.js` 配新 wasm 会报导入错误）。

---

## 3. 导入抖音开发者工具

1. 打开抖音开发者工具，登录**与小游戏 AppID 对应**的账号；
2. 新建/导入项目，目录选择：

   ```
   /Users/<you>/projects/bevy_douyin_example/platforms/douyin/package
   ```

   **不是**仓库根目录，也不是 `platforms/douyin`；
3. 项目类型选"小游戏"（目录里有 `game.json` + `project.config.json` 即会被识别）；
4. 把 `project.config.json` 里的 `appid` 改成自己的；`tt0000000000000000`
   只能跑模拟器；
5. 点"编译"。

用 CLI 也可以（需要工具已启动并允许自动化）：

```bash
douyinide check-ide-status -c OpenCode
douyinide open-project-window -c OpenCode \
  --path "$PWD/platforms/douyin/package" --type microgame
douyinide simulator-compile -c OpenCode --window-id <id>
```

---

## 4. 模拟器验收

编译成功后，模拟器里应该看到：顶部计分条、左右球拍、球与拖尾、左上角主题方块、
底部黄色广告胶囊。控制台应有类似：

```
pong: canvas 720x1561 -> world 720x1561, safe insets 0.0/185.0/0.0/62.3,
      best rally 0, wins AI 0 / player 0
```

（实际是一行日志，这里为了排版换行。数值随机型变化；顶部 inset 应该明显大于 0，
说明胶囊避让生效。）

逐项检查：

| # | 操作 | 预期 |
| --- | --- | --- |
| 1 | 上下拖动 | 右拍跟手，不越界、不穿过边框 |
| 2 | 点击左上角方块 | 主题在深/浅之间切换，**只切换一次** |
| 3 | 再点一次 | 切回，控制台各出现一条 `pong: theme ->` |
| 4 | 重开小游戏 | 主题保持上次选择 |
| 5 | 点击底部胶囊 | 无广告位时：`pong: no ad slot, using local mock` → 0.9s 后 `mock ad finished, slow-mo granted`，球明显变慢 8 秒 |
| 6 | 挂机 1.5s | 右拍自动跟球（模拟器没有指针事件时的兜底） |
| 7 | 打满 7 分 | 边框闪胜者色，2.8s 后比分归零，胜场 +1 |
| 8 | 重开小游戏 | 胜场/最高连击保留 |
| 9 | 系统切深色/浅色（工具菜单） | 未手动切换过时，游戏在约 0.5s 内跟随 |

### 截图与取色验证（可选）

```bash
douyinide miniapp-image-screenshot -c OpenCode --window-id <id>
```

模拟器截图常见为 786×1704（393×852 @2x）。游戏画布 720×1561 会被缩放显示；
不要用截图像素直接当画布坐标，先按 `screenshotScale` 和 `layoutViewport` 换算。

---

## 5. 真机验收

### 5.1 准备

1. 在抖音开放平台为你的小游戏创建**激励视频广告位**，拿到 `adUnitId`；
2. 填进 `platforms/douyin/package/game.js`：

   ```js
   var AD_UNIT_ID = 'your-ad-unit-id';
   ```

3. 重新 `./build.sh`（只改 JS 也要重新打包，确保产物一致）；
4. 用开发者工具的"真机调试/预览"扫码。

### 5.2 真机重点

| 检查项 | 说明 |
| --- | --- |
| 画布尺寸 | 长屏手机高度会大于 1561，游戏应铺满且不变形 |
| 刘海/挖孔 | 顶部计分条不与胶囊重叠；底部不贴边 |
| 胶囊避让 | 右上角菜单按钮下方应留出空间（`menuButton.bottom` 生效） |
| 触摸 | 拖动跟手；点击不穿透、不重复触发 |
| 主题 | 跟随系统设置；手动切换后不再被系统覆盖 |
| 广告 | 有广告位时应拉起真实激励视频；看完才给慢动作，中途关闭不给 |
| 存档 | 杀掉小游戏重进，胜场/纪录仍在 |
| 前后台 | 切后台再回来不黑屏、不卡死、球不飞出场地 |
| panic | 若崩溃，应弹出 `PANIC` 弹窗而不是静默退出 |

### 5.3 真机排错开关

在控制台或通过启动参数设置：

```js
GameGlobal.__probe = 1;
```

重进游戏后看日志：

```
diag: update #1 t=0.016 cams=1 sprites=27 clear=(0.035,0.047,0.086,1) win=720x1561 mem=64
```

- `win=` 应是实际画布尺寸，不是 720×1280（否则说明 `Canvas` 没读到）；
- `mem=` 应稳定，持续增长说明有泄漏；
- 真机日志可能被吞，必要时用 `tt.setStorageSync` 落盘或远程日志。

---

## 6. 发布前

- [ ] `appid` 已替换，不是占位符；
- [ ] `adUnitId` 已替换（或明确保留 mock）；
- [ ] `./build.sh` 从干净状态可复现；
- [ ] 包体 < 4MB（`du -sh platforms/douyin/package`，排除未压缩源文件）；
- [ ] 真机跑过"切后台 / 断网 / 低电量"三种情况；
- [ ] 主题、存档、广告、触摸四项回归通过；
- [ ] `git status` 里没有误提交 `pong.js` / `pong.wasm.br` / `runtime/`；
- [ ] submodule 指向 `adapter-v0.2.1`（或你验证过的新 tag）。

---

## 7. 常用命令速查

```bash
# 只做类型检查（快）
cargo check --manifest-path platforms/douyin/Cargo.toml --target wasm32-unknown-unknown

# 完整构建
./build.sh

# 不压缩 JS（排错时保留可读堆栈）
./build.sh --no-minify

# 调整 wasm 初始内存（默认 64MiB）
INITIAL_MEMORY_BYTES=$((32*1024*1024)) ./build.sh

# 看包体
du -sh platforms/douyin/package
ls -lh platforms/douyin/package

# 开发者工具
douyinide check-ide-status -c OpenCode
douyinide get-project-windows-info -c OpenCode
douyinide simulator-compile -c OpenCode --window-id <id>
douyinide miniapp-get-console-messages -c OpenCode --window-id <id>
douyinide miniapp-image-screenshot -c OpenCode --window-id <id>
```
