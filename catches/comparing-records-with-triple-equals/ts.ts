let skipped = false;

function skipSave(): void {
  skipped = true;
}

// show
const before = { x: 1, y: 2 };
const after = { x: 1, y: 2 };

if (before === after) skipSave();
// end show
console.log("skipped", skipped);
