import instance from "@/utils/request.js";
import store from "@/store/index.js";

export const personalizationGetRequest = () => instance.post('/personalization/get', {})

export const personalizationSaveRequest = profile => instance.post('/personalization/save', profile)

export const personalizationRemoveImageRequest = kind => instance.post('/personalization/remove-image', {kind})

export const personalizationUploadImageRequest = async (kind, file) => {
  const baseUrl = import.meta.env.VITE_API_URL || ''
  const response = await fetch(`${baseUrl}/personalization/upload-image`, {
    method: 'POST',
    headers: {
      Token: store.getState().user.token,
      'X-Image-Kind': kind
    },
    body: file
  })
  return response.json()
}

export const updaterStatusRequest = () => instance.post('/updater/status', {})

export const updaterUploadRequest = async file => {
  const baseUrl = import.meta.env.VITE_API_URL || ''
  const response = await fetch(`${baseUrl}/updater/upload`, {
    method: 'POST',
    headers: {Token: store.getState().user.token},
    body: file
  })
  return response.json()
}
