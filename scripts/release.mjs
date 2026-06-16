#!/usr/bin/env node
// 一键发版：同步版本号 → 提交 → 打标签 → 推送，由 CI 自动构建并发布 DMG。
// 用法：pnpm release <版本号>   例如  pnpm release 0.2.0
import { readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";

const version = process.argv[2];

function fail(msg) {
  console.error(`\n❌ ${msg}\n`);
  process.exit(1);
}

function sh(cmd) {
  return execSync(cmd, { encoding: "utf8" }).trim();
}

// 1. 校验版本号
if (!version) fail("用法：pnpm release <版本号>，例如 pnpm release 0.2.0");
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  fail(`版本号格式不合法：${version}（应为 X.Y.Z，如 0.2.0）`);
}
const tag = `v${version}`;

// 2. 工作区必须干净，避免把无关改动一起发版
if (sh("git status --porcelain")) {
  fail("工作区有未提交的改动，请先提交或暂存后再发版");
}

// 3. 标签不能重复
if (sh("git tag --list").split("\n").includes(tag)) {
  fail(`标签 ${tag} 已存在`);
}

const branch = sh("git rev-parse --abbrev-ref HEAD");
console.log(`\n📦 准备发布 ${tag}（分支 ${branch}）\n更新版本号：`);

// 4. 同步四处版本号
function patch(file, regex, replacement) {
  const before = readFileSync(file, "utf8");
  const after = before.replace(regex, replacement);
  if (before === after) fail(`未能在 ${file} 中更新版本号（正则未命中）`);
  writeFileSync(file, after);
  console.log(`  ✓ ${file}`);
}

patch("package.json", /("version":\s*")[\d.]+(")/, `$1${version}$2`);
patch("src-tauri/tauri.conf.json", /("version":\s*")[\d.]+(")/, `$1${version}$2`);
patch("src-tauri/Cargo.toml", /^(version\s*=\s*")[\d.]+(")/m, `$1${version}$2`);
patch(
  "src-tauri/Cargo.lock",
  /(name = "backtrack"\nversion = ")[\d.]+(")/,
  `$1${version}$2`,
);

// 5. 提交 + 打标签
sh("git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock");
sh(`git commit -m "chore(发布): 发布 ${tag}"`);
sh(`git tag -a ${tag} -m "Backtrack ${tag}"`);
console.log(`\n✓ 已提交并打标签 ${tag}`);

// 6. 推送分支与标签（触发 CI）
console.log("推送中...");
sh(`git push origin ${branch}`);
sh(`git push origin ${tag}`);

console.log(`\n🚀 已推送 ${tag}，GitHub Actions 开始构建 Universal DMG。`);
console.log("   进度：仓库 → Actions；完成后见 Releases。\n");
