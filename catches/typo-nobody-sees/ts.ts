async function userName(res: Response): Promise<string> {
  // show
  const data = await res.json();
  return data.usr.name;
  // end show
}

void userName(new Response('{"user":{"name":"ada"}}')).then((n) => {
  console.log(n);
});
