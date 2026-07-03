const initialCwd = process.env.INIT_CWD;

if (initialCwd) {
  process.chdir(initialCwd);
}

await import("../src/cli.ts");
