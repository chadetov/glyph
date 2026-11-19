// show
type UnverifiedEmail = { address: string };
type VerifiedEmail = { address: string };
// end show

function sendInvoice(to: VerifiedEmail): void {
  console.log("invoice sent to", to.address);
}

const signup: UnverifiedEmail = { address: "typo@exmaple.com" };
// show
sendInvoice(signup);
// end show
