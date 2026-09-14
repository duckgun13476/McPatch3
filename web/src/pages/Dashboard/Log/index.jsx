import React, {useEffect, useRef, useState} from "react";
import {Button, Dropdown, Input, message, Modal, Popconfirm, Popover, Select, Tag, Tooltip, Upload} from "antd";
import {
  taskAddDeleteFileRequest,
  taskAddHashDeletionRequest, taskCombineRequest, taskConvertAddToHashDeletionRequest, taskPackRequest,
  taskRemoveDeleteFileRequest, taskRemoveHashDeletionRequest,
  taskRevertRequest,
  taskTestRequest,
  taskUploadRequest,
  taskStatusRequest
} from "@/api/task.js";
import {terminalFullRequest, terminalMoreRequest} from "@/api/terminal.js";
import {FileMinus2, Plus, RotateCcw, Undo2, X} from "lucide-react";
import {generateRandomStr, showFileSize, showTime} from "@/utils/tool.js";
import {miscVersionListRequest} from "@/api/misc.js";
import {hasVersionWhitespace, nextPatchVersion} from "@/utils/version.js";

const {TextArea} = Input;

const VersionList = ({versionList}) => {

  const content = (
    <div>
      {
        versionList.slice(0, 10).map((version, index) => {
          return (
            <div key={index} className={"p-2"}>
              <div className={"flex justify-start"}>
                <div className={"mr-3 w-40 max-h-4"}><span className={"font-bold"}>版本号: </span>{version.label}</div>
                <div className={"mr-3 w-40 max-h-4"}>
                  <span className={"font-bold"}>大小: </span>{showFileSize(version.size)}
                </div>
                <div className={"mr-3 w-96 max-h-12 overflow-auto"}>
                  <span className={"font-bold"}>更新记录: </span>{version.change_logs}
                </div>
              </div>
            </div>
          )
        })
      }
    </div>
  )

  return (
    <>
      {
        versionList.length > 0 ? (
          <Popover placement="bottom" content={content}>
            <Button size={"large"}>{versionList[0].label}</Button>
          </Popover>
        ) : (
          <></>
        )
      }
    </>
  )
}

const Index = () => {

  const options = [
    {value: 3000, label: '3s'},
    {value: 1000, label: '1s'},
    {value: 5000, label: '5s'},
    {value: 10000, label: '10s'},
  ]

  const [logs, setLogs] = useState([])
  const [packShow, setPackShow] = useState(false)
  const [version, setVersion] = useState('');
  const [updateRecord, setUpdateRecord] = useState('');
  const [refreshInterval, setRefreshInterval] = useState(parseInt(localStorage.getItem('logRefreshInterval')) || options[0].value);
  const [versionList, setVersionList] = useState([])
  const [packPreview, setPackPreview] = useState(null)
  const [excludedChangeIds, setExcludedChangeIds] = useState([])
  const [deletePath, setDeletePath] = useState('')
  const [hashDeletePath, setHashDeletePath] = useState('')
  const [packLoading, setPackLoading] = useState(false)
  const [hashDeleteLoading, setHashDeleteLoading] = useState(false)
  const logsRef = useRef(null);
  const [messageApi, contextHolder] = message.useMessage();

  useEffect(() => {
    terminalFull()
  }, []);

  useEffect(() => {
    if (logsRef.current) {
      logsRef.current.scrollTop = logsRef.current.scrollHeight;
    }
  }, [logs]);

  useEffect(() => {
    const intervalId = setInterval(() => {
      terminalMore()
    }, refreshInterval)

    return () => clearInterval(intervalId);
  }, [refreshInterval])

  useEffect(() => {
    miscVersionList()
  }, []);

  const terminalFull = async () => {
    const {code, msg, data} = await terminalFullRequest();
    if (code === 1) {
      setLogs(data.content)
    }
  }

  const terminalMore = async () => {
    const {code, msg, data} = await terminalMoreRequest();
    if (code === 1) {
      if (data.content.length !== 0) {
        setLogs(prev => prev.concat(data.content))
      }
    }
  }

  const changeRefreshInterval = (value) => {
    setRefreshInterval(value)
    localStorage.setItem('logRefreshInterval', value)
  };

  const miscVersionList = async () => {
    const {code, msg, data} = await miscVersionListRequest();
    if (code === 1) {
      setVersionList(data.versions);
    }
  }

  const loadPackPreview = async (tempVersion, tempUpdateRecord) => {
    const {code, msg, data} = await taskPackRequest(tempVersion, tempUpdateRecord)
    if (code !== 1) {
      messageApi.error(msg)
      return false
    }
    setPackPreview(data)
    setExcludedChangeIds([])
    return true
  }

  const taskPack = async () => {
    if (version === '') {
      messageApi.error('无法生成版本号，请刷新版本列表后重试。')
      return
    }
    if (hasVersionWhitespace(version.trim())) {
      messageApi.error('版本号不能包含内部空白字符。')
      return
    }

    const tempVersion = version
    const tempUpdateRecord = updateRecord === '' ? '这个人很懒, 没有写更新记录.' : updateRecord

    setPackLoading(true)
    try {
      if (packPreview === null) {
        await loadPackPreview(tempVersion, tempUpdateRecord)
        return
      }

      const {code, msg} = await taskPackRequest(
        tempVersion,
        tempUpdateRecord,
        packPreview.fingerprint,
        excludedChangeIds
      )
      if (code === 1) {
        messageApi.success('打包任务已提交。')
        closePackDialog()
      } else {
        messageApi.error(msg)
        setPackPreview(null)
        setExcludedChangeIds([])
      }
    } finally {
      setPackLoading(false)
    }
  }

  const addDeleteChange = async () => {
    if (deletePath.trim() === '') {
      messageApi.warning('请输入需要从客户端删除的相对路径。')
      return
    }
    const {code, msg} = await taskAddDeleteFileRequest(deletePath.trim())
    if (code !== 1) {
      messageApi.error(msg)
      return
    }
    setDeletePath('')
    setPackPreview(null)
    setExcludedChangeIds([])
    messageApi.success('已加入客户端删除指令。')
  }

  const removeChange = async (change) => {
    if (change.explicit && change.operation === 'delete-file-by-hash') {
      const {code, msg} = await taskRemoveHashDeletionRequest(change.path)
      if (code !== 1) {
        messageApi.error(msg)
        return
      }
      const tempVersion = version === '' ? generateRandomStr() : version
      const tempUpdateRecord = updateRecord === '' ? '这个人很懒, 没有写更新记录.' : updateRecord
      await loadPackPreview(tempVersion, tempUpdateRecord)
      return
    }
    if (change.explicit && change.operation === 'delete-file') {
      const {code, msg} = await taskRemoveDeleteFileRequest(change.path)
      if (code !== 1) {
        messageApi.error(msg)
        return
      }
      const tempVersion = version === '' ? generateRandomStr() : version
      const tempUpdateRecord = updateRecord === '' ? '这个人很懒, 没有写更新记录.' : updateRecord
      await loadPackPreview(tempVersion, tempUpdateRecord)
      return
    }
    setExcludedChangeIds(current => current.includes(change.id) ? current : [...current, change.id])
  }

  const closePackDialog = () => {
    setPackShow(false)
    setPackPreview(null)
    setExcludedChangeIds([])
    setDeletePath('')
    setHashDeletePath('')
  }

  const operationLabel = (operation) => ({
    'create-directory': '新增目录',
    'add-file': '新增文件',
    'update-file': '替换文件',
    'move-file': '移动',
    'delete-file': '删除文件',
    'delete-directory': '删除目录',
    'delete-file-by-hash': '客户端哈希删除'
  }[operation] || operation)

  const operationColor = (operation) => ({
    'create-directory': 'green',
    'add-file': 'cyan',
    'update-file': 'blue',
    'move-file': 'gold',
    'delete-file': 'red',
    'delete-directory': 'red',
    'delete-file-by-hash': 'magenta'
  }[operation] || 'default')

  const visiblePackChanges = packPreview?.changes.filter(change => !excludedChangeIds.includes(change.id)) || []

  const refreshPackPreview = async () => {
    const tempUpdateRecord = updateRecord === '' ? '这个人很懒, 没有写更新记录.' : updateRecord
    await loadPackPreview(version, tempUpdateRecord)
  }

  const changeContextItems = (change) => {
    if (change.operation === 'add-file' && !change.explicit) {
      return [{key: 'hash-delete', label: '改为客户端哈希删除'}]
    }
    if (change.operation === 'delete-file-by-hash' && change.explicit) {
      return [{key: 'restore-add', label: '撤销哈希删除并恢复新增'}]
    }
    return []
  }

  const handleChangeContextAction = async (change, key) => {
    if (key === 'hash-delete') {
      const {code, msg} = await taskConvertAddToHashDeletionRequest(change.path)
      if (code !== 1) {
        messageApi.error(msg)
        return
      }
      messageApi.success('已改为精确路径哈希删除。')
      await refreshPackPreview()
    } else if (key === 'restore-add') {
      const {code, msg} = await taskRemoveHashDeletionRequest(change.path)
      if (code !== 1) {
        messageApi.error(msg)
        return
      }
      messageApi.success('已撤销哈希删除并恢复新增文件。')
      await refreshPackPreview()
    }
  }

  const hashDeleteUploadProps = {
    showUploadList: false,
    multiple: false,
    maxCount: 1,
    customRequest: async ({file, onSuccess, onError, onProgress}) => {
      if (hashDeletePath.trim() === '') {
        messageApi.warning('请先填写玩家客户端中的精确相对路径。')
        onError(new Error('missing target path'))
        return
      }
      setHashDeleteLoading(true)
      try {
        const response = await taskAddHashDeletionRequest(file, hashDeletePath.trim(), onProgress)
        if (response.code !== 1) {
          messageApi.error(response.msg)
          onError(new Error(response.msg))
          return
        }
        setPackPreview(null)
        setExcludedChangeIds([])
        setHashDeletePath('')
        messageApi.success(`已登记精确路径哈希删除：${file.name}`)
        onSuccess(response)
      } catch (error) {
        messageApi.error('客户端删除指纹上传失败。')
        onError(error)
      } finally {
        setHashDeleteLoading(false)
      }
    }
  }

  const taskCombine = async () => {
    const {code, msg, data} = await taskCombineRequest();
    if (code === 1) {
      messageApi.success('合并成功.')
    } else {
      messageApi.error(msg)
    }
  }

  const taskTest = async () => {
    const {code, msg, data} = await taskTestRequest();
    if (code === 1) {
      messageApi.success('测试成功.')
    } else {
      messageApi.error(msg)
    }
  }

  const taskRevert = async () => {
    const {code, msg, data} = await taskRevertRequest();
    if (code === 1) {
      messageApi.success('回退成功.')
    } else {
      messageApi.error(msg)
    }
  }

  const taskUpload = async () => {
    const {code, msg, data} = await taskUploadRequest();
    if (code === 1) {
      messageApi.success('任务已提交.')
    } else {
      messageApi.error(msg)
    }
  }

  const taskStatus = async () => {
    const {code, msg, data} = await taskStatusRequest();
    if (code === 1) {
      messageApi.success('任务已提交.')
    } else {
      messageApi.error(msg)
    }
  }

  const copy = async (item) => {
    await navigator.clipboard.writeText(`${showTime(item.time)}-${item.level}-${item.content}`);
    messageApi.success('复制成功!')
  }

  const getTextColor = (level) => {
    if (level === 'debug') return 'text-zinc-500';
    if (level === 'info') return 'text-white';
    if (level === 'warning') return 'text-yellow-600';
    if (level === 'error') return 'text-[#FF0000]';
  };

  return (
    <>
      {contextHolder}
      <div className="flex flex-col p-10 min-h-screen">
        <div className="flex justify-start items-center h-8">
          <VersionList versionList={versionList}/>
          <Popconfirm title="风险操作,请再次确认!" onConfirm={taskStatus} okText="确定" cancelText="取消">
            <Button type="primary" size="large" className="ml-2">检查文件修改</Button>
          </Popconfirm>
          <Popconfirm title="风险操作,请再次确认!" onConfirm={taskTest} okText="确定" cancelText="取消">
            <Button type="primary" size="large" className="ml-2">测试更新包</Button>
          </Popconfirm>
          <Popconfirm title="风险操作,请再次确认!" onConfirm={taskUpload} okText="确定" cancelText="取消">
            <Button type="primary" size="large" className="ml-2">上传public目录</Button>
          </Popconfirm>
          <Button type="primary" size="large" className="ml-2" onClick={() => {
            const nextVersion = nextPatchVersion(versionList[0]?.label)
            if (nextVersion === '') {
              messageApi.error('无法从最新版本生成下一个版本号。')
              return
            }
            setVersion(nextVersion)
            setPackShow(true)
          }}>打包新版本</Button>
          <Popconfirm title="风险操作,请再次确认!" onConfirm={taskRevert} okText="确定" cancelText="取消">
            <Button type="primary" size="large" className="ml-2">回退整个工作空间</Button>
          </Popconfirm>
          <Popconfirm title="风险操作,请再次确认!" onConfirm={taskCombine} okText="确定" cancelText="取消">
            <Button type="primary" size="large" className="ml-2">合并更新包</Button>
          </Popconfirm>
          <Select
            defaultValue={refreshInterval}
            size={"large"}
            className="ml-auto w-40"
            onChange={changeRefreshInterval}
            options={options}/>
          <Button type="primary" size="large" className="ml-2" icon={<RotateCcw size={20} strokeWidth={1.5}/>}
                  onClick={terminalMore}/>

        </div>
        <div
          ref={logsRef}
          className="flex-1 mt-8 bg-black dark:bg-gray-800 text-white overflow-auto min-h-[calc(100vh-160px)] max-h-[calc(100vh-160px)]">
          {
            logs.map((item, index) => {
              return (
                <div
                  key={index}
                  onClick={() => copy(item)}
                  className="flex items-center pt-0.5 pb-0.5 pl-2 text-base text-gray-300 rounded cursor-pointer select-none hover:bg-gray-700 duration-200">
                  <span className="w-48">[{showTime(item.time)}]</span>
                  {/*<span className={`w-24 ${getTextColor(item.level)}`}>[{item.level}]</span>*/}
                  <span className={`${getTextColor(item.level)}`}>{item.content}</span>
                </div>
              )
            })
          }
        </div>
      </div>
      <Modal
        title={packPreview === null ? "打包新版本" : "确认本次文件变化"}
        width={780}
        okText={packPreview === null ? "查看变化" : "确认并打包"}
        cancelText="取消"
        open={packShow}
        confirmLoading={packLoading}
        okButtonProps={{disabled: packPreview !== null && visiblePackChanges.length === 0}}
        onOk={taskPack}
        onCancel={closePackDialog}>
        {packPreview === null ? (
          <div>
            <div className="text-base text-gray-400">版本号已自动递增；首次确认只生成变化预览，不会立即打包。</div>
            <Input
              className="mt-5"
              placeholder="版本号已按最新版本自动递增。"
              value={version}
              onChange={(e) => setVersion(e.target.value)}/>
            <TextArea
              className="mt-2"
              placeholder="请输入更新记录。"
              autoSize={{maxRows: 10, minRows: 4}}
              maxLength={4000}
              value={updateRecord}
              onChange={(e) => setUpdateRecord(e.target.value)}/>
            <div className="mt-5 text-sm font-medium text-gray-700">直接删除客户端文件</div>
            <div className="mt-2 flex gap-2">
              <Input
                placeholder="例如 .minecraft/mods/old-version.jar"
                value={deletePath}
                onPressEnter={addDeleteChange}
                onChange={(e) => setDeletePath(e.target.value)}/>
              <Tooltip title="加入删除指令">
                <Button icon={<Plus size={18}/>} onClick={addDeleteChange}/>
              </Tooltip>
            </div>
            <div className="mt-5 text-sm font-medium text-gray-700">按精确路径和文件哈希删除客户端文件</div>
            <div className="mt-2 text-xs text-gray-400">
              填写玩家客户端中的相对路径并上传原文件作为指纹。服务端不保存上传内容，客户端仅在路径、大小和 SHA-256 全部一致时删除。
            </div>
            <div className="mt-2 flex gap-2">
              <Input
                placeholder="例如 .minecraft/mods/obsolete.jar"
                value={hashDeletePath}
                onChange={(e) => setHashDeletePath(e.target.value)}/>
              <Upload {...hashDeleteUploadProps}>
                <Button icon={<FileMinus2 size={18}/>} loading={hashDeleteLoading}>选择原文件</Button>
              </Upload>
            </div>
          </div>
        ) : (
          <div>
            <div className="mb-3 flex items-center justify-between text-sm text-gray-500">
              <span>共 {packPreview.changes.length} 项，已排除 {excludedChangeIds.length} 项</span>
              {excludedChangeIds.length > 0 && (
                <Button type="text" icon={<Undo2 size={16}/>} onClick={() => setExcludedChangeIds([])}>恢复全部</Button>
              )}
            </div>
            <div className="max-h-[430px] overflow-y-auto pr-1">
              {visiblePackChanges.map(change => {
                const contextItems = changeContextItems(change)
                const card = (
                <div className="relative mb-2 rounded-md border border-gray-200 px-3 py-3 pr-12">
                  <div className="flex items-center gap-2">
                    <Tag color={operationColor(change.operation)}>{operationLabel(change.operation)}</Tag>
                    {change.explicit && <Tag>显式指令</Tag>}
                  </div>
                  <div className="mt-2 break-all font-mono text-sm text-gray-800">
                    {change.operation === 'move-file' ? `${change.from} -> ${change.to}` : change.path}
                  </div>
                  {(change.hash || change.len !== null) && (
                    <div className="mt-1 break-all text-xs text-gray-400">
                      {change.len !== null && `${showFileSize(change.len)} `}{change.hash || ''}
                    </div>
                  )}
                  <Tooltip title={change.explicit ? "撤销这条删除指令" : "本次不打包此项"}>
                    <Button
                      type="text"
                      danger
                      aria-label="移除此项变化"
                      className="absolute right-2 top-2"
                      icon={<X size={18}/>}
                      onClick={() => removeChange(change)}/>
                  </Tooltip>
                </div>
                )
                return contextItems.length > 0 ? (
                  <Dropdown
                    key={change.id}
                    trigger={['contextMenu']}
                    menu={{items: contextItems, onClick: ({key}) => handleChangeContextAction(change, key)}}>
                    {card}
                  </Dropdown>
                ) : <React.Fragment key={change.id}>{card}</React.Fragment>
              })}
              {visiblePackChanges.length === 0 && (
                <div className="py-12 text-center text-gray-400">没有选中的文件变化</div>
              )}
            </div>
          </div>
        )}
      </Modal>
    </>
  );
};

export default Index;
