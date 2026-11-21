function save(id: string): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(() => {
      console.log("saved", id);
      resolve();
    }, 10);
  });
}

function saveAll(ids: string[]): void {
  // show
  ids.forEach(async (id) => { await save(id); });
  // end show
  console.log("done");
}

saveAll(["a", "b"]);
