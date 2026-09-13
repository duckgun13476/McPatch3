import instance from "@/utils/request.js";

export const taskPackRequest = (label, changeLogs, confirmationFingerprint = null, excludedChangeIds = []) => instance.post('/task/pack', {
  label: label,
  change_logs: changeLogs,
  confirmation_fingerprint: confirmationFingerprint,
  excluded_change_ids: excludedChangeIds
})

export const taskAddDeleteFileRequest = (path) => instance.post('/task/change/delete-file', {path})

export const taskRemoveDeleteFileRequest = (path) => instance.post('/task/change/remove-delete-file', {path})

export const taskCombineRequest = () => instance.post('/task/combine', {})

export const taskTestRequest = () => instance.post('/task/test', {})

export const taskRevertRequest = () => instance.post('/task/revert', {})

export const taskUploadRequest = () => instance.post('/task/upload', {})

export const taskStatusRequest = () => instance.post('/task/status', {})
