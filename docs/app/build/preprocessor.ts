import "./build.ts";

if (Deno.args[0] === "supports") {
  Deno.exit(1);
}

const [, book] = await read(Deno.stdin.readable).then(JSON.parse);

console.log(JSON.stringify(book));

async function read(r: ReadableStream): Promise<string> {
  const reader = r.getReader();
  const decoder = new TextDecoder();
  let result = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    result += decoder.decode(value);
  }
  return result;
}
