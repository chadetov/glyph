function refundable(status: "paid" | "shipped" | "delivered"): boolean {
  switch (status) {
    // show
    case "paid": return true;
    case "shipped": return true;
    case "paid": return false; // never runs
    // end show
    case "delivered": return false;
  }
}

console.log(refundable("paid"));
