// 抖音小游戏入口。
//
// 适配层（addons/bevy-douyin/runtime）负责安装 DOM/WebGL2/输入 shim，
// 然后加载 wasm-bindgen 生成的 pong.js。构建脚本只会覆盖 pong.js /
// pong.wasm.br / runtime/，不会改这个文件。
var runtime = require('./runtime/bootstrap.js');

// 留空 = 没有真实广告位，适配层 0.2.2 起会内置 mock（默认 600ms 后返回 ended）。
// 0.3 起用 RequestAd 返回 ticket、AdResult 回传同一张 ticket，Rust 侧按票据发奖。
// 适配层 30s 内没有终态会主动报 timeout；游戏仍保留 RequestAd==0 时的 0.9s
// 本地兜底（见 src/game.rs 的 AdPhase::Mock）。
// 换成真实激励视频广告位后，Rust 侧不需要改动。
var AD_UNIT_ID = '';

runtime.start({
  name: 'pong',
  canvas: '#pong-canvas',
  // 设计分辨率（逻辑像素）。适配层会按屏幕宽高比推导实际画布：
  // 竖屏高手机保持宽度 720、增高；平板等宽屏保持高度 1280、增宽。
  width: 720,
  height: 1280,
  wasm: 'pong.wasm.br',
  adUnitId: AD_UNIT_ID,
  globalPrefix: '__bevy_douyin_', // 必须和 host.rs 的 GLOBAL_PREFIX 一致
  // 存档命名空间：游戏侧 key 为 best_rally，实际写入 pong.best_rally；
  // clear() 只清这个前缀，不会碰 __boot_notes 或其他数据。
  storagePrefix: 'pong.',
  glue: function () { return require('./pong.js'); }
});
