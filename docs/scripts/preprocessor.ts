import process from "node:process";
import Stream from "node:stream";

if (process.argv[2] === "supports") {
  process.exit(process.argv[3] === "html" ? 0 : 1);
}

await import("./build.ts");

const [, book] = await read(process.stdin).then(JSON.parse);

console.log(JSON.stringify(book));

async function read(r: NodeJS.ReadStream): Promise<string> {
  const decoder = new TextDecoderStream();
  r.pipe(Stream.Writable.fromWeb(decoder.writable));
  const reader = decoder.readable.getReader();
  let result = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    result += value;
  }
  return result;
}
