import { login } from "./auth-grpc.js";

const form = document.querySelector("#login-form");
const status = document.querySelector("#status");
const output = document.querySelector("#output");

function setStatus(message, ok) {
  status.textContent = message;
  status.className = ok ? "ok" : "error";
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  const email = form.email.value.trim();
  const password = form.password.value;
  const submit = form.querySelector("button");

  submit.disabled = true;
  setStatus("Calling gRPC Login...", true);
  output.textContent = "";

  try {
    const result = await login(email, password);
    localStorage.setItem("friday.auth.token", result.token);
    setStatus("Login succeeded", true);
    output.textContent = JSON.stringify(result, null, 2);
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), false);
  } finally {
    submit.disabled = false;
  }
});
