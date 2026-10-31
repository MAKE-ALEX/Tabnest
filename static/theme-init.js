/* Apply the saved appearance before the first paint to avoid a flash. */
(function () {
  'use strict';
  try {
    var params = new URLSearchParams(window.location.search);
    var palettes = ['clay', 'moss', 'ocean', 'plum', 'sakura', 'amber'];
    var storedPalette = localStorage.getItem('tabnest.palette');
    var requestedPalette = params.get('palette');
    var palette = palettes.indexOf(requestedPalette) >= 0
      ? requestedPalette
      : (palettes.indexOf(storedPalette) >= 0 ? storedPalette : 'clay');
    document.documentElement.dataset.palette = palette;

    var stored = localStorage.getItem('tabnest.theme');
    var requested = params.get('theme');
    var preference = (requested === 'dark' || requested === 'light' || requested === 'auto')
      ? requested
      : ((stored === 'dark' || stored === 'light' || stored === 'auto') ? stored : 'auto');
    var theme = preference === 'auto'
      ? (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
      : preference;
    document.documentElement.dataset.theme = theme;
  } catch (error) {
    /* localStorage may be unavailable; the defaults still work */
  }
})();
