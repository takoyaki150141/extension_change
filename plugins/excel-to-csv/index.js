module.exports = function (ctx) {
  for (let i = 0; i < ctx.files.length; i++) {
    const file = ctx.files[i];
    const out = ctx.excelToCsv(file);
    ctx.log(file.name + " -> " + out);
    ctx.progress((i + 1) / ctx.files.length);
  }
};
