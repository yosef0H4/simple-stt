import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import "./classic.css";
mount(App, { target: document.getElementById("app")! });
