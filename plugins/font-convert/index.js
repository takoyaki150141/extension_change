module.exports = function (ctx) {
  const target = String(ctx.action || "").toLowerCase();

  if (ctx.files.length === 0) {
    ctx.log("変換するファイルがありません。");
    return;
  }

  for (let i = 0; i < ctx.files.length; i++) {
    const f = ctx.files[i];
    const out = ctx.font(f, target, {});
    ctx.log(f.name + " -> " + out);
    ctx.progress((i + 1) / ctx.files.length);
  }
};
