import instance from "@/utils/request.js";
import axios from "axios";
import store from "@/store/index.js";

export const taskPackRequest = (label, changeLogs, confirmationFingerprint = null, excludedChangeIds = []) => instance.post('/task/pack', {
  label: label,
  change_logs: changeLogs,
  confirmation_fingerprint: confirmationFingerprint,
  excluded_change_ids: excludedChangeIds
})

export const taskPackUpdaterRequest = (label, changeLogs = '更新自动更新器') => instance.post('/task/pack-updater', {
  label,
  change_logs: changeLogs
})

export const taskAddDeleteFileRequest = (path) => instance.post('/task/change/delete-file', {path})

export const taskRemoveDeleteFileRequest = (path) => instance.post('/task/change/remove-delete-file', {path})

export const taskAddHashDeletionRequest = (file, path, onProgress) => axios.post(
  `${import.meta.env.VITE_API_URL}/task/change/delete-file-by-hash`,
  file,
  {
    headers: {
      'Token': store.getState().user.token,
      'Content-Type': 'application/octet-stream',
      'File-Path': encodeURIComponent(path)
    },
    onUploadProgress: (event) => {
      if (event.total) onProgress?.({percent: Math.floor((event.loaded / event.total) * 100)})
    }
  }
).then(response => response.data)

export const taskConvertAddToHashDeletionRequest = (path) => instance.post(
  '/task/change/convert-add-to-hash-delete',
  {path}
)

export const taskRemoveHashDeletionRequest = (path) => instance.post(
  '/task/change/remove-delete-file-by-hash',
  {path}
)

export const taskCombineRequest = () => instance.post('/task/combine', {})

export const taskTestRequest = () => instance.post('/task/test', {})

export const taskRevertRequest = () => instance.post('/task/revert', {})

export const taskUploadRequest = () => instance.post('/task/upload', {})

export const taskStatusRequest = () => instance.post('/task/status', {})
