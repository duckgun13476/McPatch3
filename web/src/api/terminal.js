import instance from "@/utils/request.js";
import store from "@/store/index.js";

export const terminalFullRequest = () => instance.post('/terminal/full', {})

export const terminalMoreRequest = () => instance.post('/terminal/more', {})

export const terminalStreamRequest = (signal) => fetch(`${import.meta.env.VITE_API_URL}/terminal/stream`, {
  method: 'GET',
  headers: {Token: store.getState().user.token},
  signal
})
