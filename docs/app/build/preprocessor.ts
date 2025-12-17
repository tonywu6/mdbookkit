if (Deno.args[0] === "supports") {
  Deno.exit(Deno.args[1] === "html" ? 0 : 1);
}

await import("./build.ts");

const [, book] = await read(Deno.stdin.readable).then(JSON.parse);

console.log(JSON.stringify(book));

async function read(r: ReadableStream): Promise<string> {
  const decoder = new TextDecoderStream();
  r.pipeTo(decoder.writable);
  const reader = decoder.readable.getReader();
  let result = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    result += value;
  }
  return result;
}
