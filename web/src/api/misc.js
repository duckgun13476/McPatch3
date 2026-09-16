import instance from "@/utils/request.js";

export const miscVersionListRequest = () => instance.post('/misc/version-list', {})

export const miscVersionHistoryRequest = label => instance.post('/misc/version-history', {label})
