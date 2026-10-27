module.exports = function (ctx) {
  const action = String(ctx.action || "").toLowerCase();
  if (ctx.files.length === 0) {
    ctx.log("変換するファイルがありません。");
    return;
  }

  for (let i = 0; i < ctx.files.length; i++) {
    const file = ctx.files[i];
    let out;
    if (action === "md2html") {
      out = ctx.markdownToHtml(file);
    } else if (action === "md2epub") {
      out = ctx.markdownToEpub(file);
    } else {
      throw new Error("unknown action: " + action);
    }
    ctx.log(file.name + " -> " + out);
    ctx.progress((i + 1) / ctx.files.length);
  }
};
