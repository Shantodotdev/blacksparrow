import { NextResponse } from "next/server";
import fs from "node:fs";
import path from "node:path";

export async function GET() {
  const filePath = path.resolve(process.cwd(), "../docs/assets/logo.png");
  if (!fs.existsSync(filePath)) {
    return new NextResponse("Not found", { status: 404 });
  }
  const buffer = fs.readFileSync(filePath);
  return new NextResponse(buffer, {
    headers: {
      "Content-Type": "image/png",
      "Cache-Control": "public, max-age=31536000, immutable",
    },
  });
}
