/* 在首帧之前应用已保存的外观，避免出现主题闪烁（FOUC）。
   这个脚本在 <head> 里同步执行，因此不能用模块或 async。 */
(function () {
  'use strict';
  try {
    var params = new URLSearchParams(window.location.search);
    // 与 app.js 中的 PALETTES 保持一致；这里只关心 id，不需要显示名。
    var palettes = ['clay', 'moss', 'ocean', 'plum', 'sakura', 'amber'];
    var storedPalette = localStorage.getItem('tabnest.palette');
    var requestedPalette = params.get('palette');
    // 优先级：URL 参数 > localStorage > 默认「陶土」。
    var palette = palettes.indexOf(requestedPalette) >= 0
      ? requestedPalette
      : (palettes.indexOf(storedPalette) >= 0 ? storedPalette : 'clay');
    document.documentElement.dataset.palette = palette;

    var stored = localStorage.getItem('tabnest.theme');
    var requested = params.get('theme');
    // 同样先看 URL 参数，再看保存值；非法值一律退回 auto。
    var preference = (requested === 'dark' || requested === 'light' || requested === 'auto')
      ? requested
      : ((stored === 'dark' || stored === 'light' || stored === 'auto') ? stored : 'auto');
    // auto 时实时读取系统偏好，得到最终要写入的 dark / light。
    var theme = preference === 'auto'
      ? (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
      : preference;
    document.documentElement.dataset.theme = theme;
  } catch (error) {
    /* localStorage 可能被禁用；此时保留 CSS 里的默认外观即可 */
  }
})();
