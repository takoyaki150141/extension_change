module.exports = function (ctx) {
  const target = String(ctx.action || "").toLowerCase();
  const aliases = {
    jpg: ["jpg", "jpeg"],
    tiff: ["tif", "tiff"],
  };
  const targetExts = aliases[target] || [target];
  const files = ctx.files.filter(function (f) {
    return targetExts.indexOf(String(f.ext || "").toLowerCase()) < 0;
  });

  if (files.length === 0) {
    ctx.log("変換するファイルがありません。");
    return;
  }

  for (let i = 0; i < files.length; i++) {
    const f = files[i];
    const out = ctx.convert(f, target, { quality: 92 });
    ctx.log(f.name + " -> " + out);
    ctx.progress((i + 1) / files.length);
  }
};
