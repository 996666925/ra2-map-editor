// CommonJS loader for SWC output. All modules are Rust-owned virtual files.
// No process, filesystem, network or other host APIs are installed in Boa.
function __yrExecute(outputs) {
  const cache = Object.create(null);
  function load(name) {
    if (Object.prototype.hasOwnProperty.call(cache, name)) return cache[name].exports;
    if (!Object.prototype.hasOwnProperty.call(outputs, name)) {
      throw new Error("Module is outside this mission project: " + name);
    }
    const module = { exports: {} };
    cache[name] = module;
    const require = specifier => {
      if ((!specifier.startsWith("./") && !specifier.startsWith("../")) || specifier.includes("\\")) {
        throw new Error("Only project-relative imports are supported");
      }
      const parts = name.split("/"); parts.pop();
      for (const part of specifier.split("/")) {
        if (part === "..") {
          if (parts.length <= 1) throw new Error("Import escapes project");
          parts.pop();
        } else if (part !== "." && part !== "") parts.push(part);
      }
      let resolved = parts.join("/").replace(/\.ts$/, ".js");
      if (!resolved.endsWith(".js")) resolved += ".js";
      return load(resolved);
    };
    new Function("exports", "require", "module", outputs[name])(module.exports, require, module);
    return module.exports;
  }
  return JSON.stringify(load("/mission.js").default);
}
