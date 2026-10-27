module.exports = function (ctx) {
  const target = String(ctx.action || "").toLowerCase();
  const files = ctx.files.filter(function (f) {
    return String(f.ext || "").toLowerCase() !== target;
  });

  if (files.length === 0) {
    ctx.log("変換するファイルがありません。");
    return;
  }

  for (let i = 0; i < files.length; i++) {
    const f = files[i];
    const out = ctx.mesh(f, target, {});
    ctx.log(f.name + " -> " + out);
    ctx.progress((i + 1) / files.length);
  }
};
