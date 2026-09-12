const fs = require('fs');

const wasmBytes = fs.readFileSync('main.wasm');
let memory;
const hostCalls = [];

function text(ptr, len) {
  return new TextDecoder('utf-8').decode(
    new Uint8Array(memory.buffer, Number(ptr), Number(len)),
  );
}

const imports = {
  env: {
    js_log: (ptr, len) => hostCalls.push(['js_log', text(ptr, len)]),
    js_set_text: (id, idLen, value, valueLen) =>
      hostCalls.push(['js_set_text', text(id, idLen), text(value, valueLen)]),
    js_append_text: (id, idLen, value, valueLen) =>
      hostCalls.push(['js_append_text', text(id, idLen), text(value, valueLen)]),
    js_set_badge_color: (id, idLen, color, colorLen) =>
      hostCalls.push(['js_set_badge_color', text(id, idLen), text(color, colorLen)]),
  },
};

(async () => {
  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
  memory = instance.exports.memory;
  const initialPages = memory.buffer.byteLength / 65536;

  instance.exports.InitApp();
  const addResult = Number(instance.exports.AddNumbers(1234, 5678));
  const fibResult = Number(instance.exports.RunComputation(10));
  instance.exports.AppendLogMessage(1);

  const result = {
    instantiated: true,
    initial_memory_pages: initialPages,
    final_memory_pages: memory.buffer.byteLength / 65536,
    add_1234_5678: addResult,
    fib_10: fibResult,
    host_calls: hostCalls,
  };
  console.log(JSON.stringify(result));

  if (addResult !== 6912 || fibResult !== 55) {
    process.exitCode = 1;
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
