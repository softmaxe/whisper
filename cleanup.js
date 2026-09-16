const fs = require("fs");
const path = require("path");

const dirsToClean = ["dist", "src/dist", "node_modules/.cache"];

dirsToClean.forEach((dir) => {
  fs.rmSync(path.join(__dirname, dir), { recursive: true, force: true });
  console.log(`Cleaned: ${dir}`);
});
