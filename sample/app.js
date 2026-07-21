const button = document.getElementById("counter");
let clicks = 0;

button.addEventListener("click", () => {
  clicks += 1;
  button.textContent = `Clicked ${clicks} time${clicks === 1 ? "" : "s"}`;
});
