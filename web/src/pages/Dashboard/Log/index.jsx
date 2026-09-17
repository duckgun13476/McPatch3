import React, {useEffect, useRef, useState} from "react";
import {Button, Dropdown, Input, message, Modal, Popconfirm, Popover, Progress, Segmented, Tag, Tooltip, Upload} from "antd";
import {
  taskAddDeleteFileRequest,
  taskAddHashDeletionRequest, taskCombineRequest, taskConvertAddToHashDeletionRequest, taskPackRequest,
  taskRemoveDeleteFileRequest, taskRemoveHashDeletionRequest,
  taskRevertRequest,
  taskTestRequest,
  taskUploadRequest
} from "@/api/task.js";
import {terminalStreamRequest} from "@/api/terminal.js";
import {ArrowLeft, FileMinus2, Grid2X2, List, PanelRightClose, PanelRightOpen, Plus, RotateCcw, Undo2, X} from "lucide-react";
import {generateRandomStr, showFileSize, showTime} from "@/utils/tool.js";
import {miscVersionListRequest} from "@/api/misc.js";
import {hasVersionWhitespace, nextVersionForHistory} from "@/utils/version.js";
import {fsDiskInfoRequest, fsListRequest} from "@/api/fs.js";
import FileBreadcrumb from "@/components/FileBreadcrumb/index.jsx";
import FolderButtonGroup from "@/components/FolderButtonGroup/index.jsx";
import TileViewFileExplorer from "@/components/TileViewFileExplorer/index.jsx";

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
  const [logs, setLogs] = useState([])
  const [packShow, setPackShow] = useState(false)
  const [version, setVersion] = useState('');
  const [updateRecord, setUpdateRecord] = useState('');
  const [versionList, setVersionList] = useState([])
  const [packPreview, setPackPreview] = useState(null)
  const [excludedChangeIds, setExcludedChangeIds] = useState([])
  const [deletePath, setDeletePath] = useState('')
  const [hashDeletePath, setHashDeletePath] = useState('')
  const [packLoading, setPackLoading] = useState(false)
  const [hashDeleteLoading, setHashDeleteLoading] = useState(false)
  const [diskInfo, setDiskInfo] = useState({total: 0, used: 0, workspace_used: 0, workspace_files: 0, workspace_path: '', public_used: 0, public_files: 0})
  const [path, setPath] = useState(JSON.parse(localStorage.getItem('filePath')) || [])
  const [fileList, setFileList] = useState([])
  const [viewMode, setViewMode] = useState(localStorage.getItem('fileViewMode') || 'grid')
  const [logCollapsed, setLogCollapsed] = useState(localStorage.getItem('logCollapsed') === 'true')
  const [streamState, setStreamState] = useState('connecting')
  const [streamEpoch, setStreamEpoch] = useState(0)
  const [autoFollow, setAutoFollow] = useState(true)
  const logsRef = useRef(null);
  const [messageApi, contextHolder] = message.useMessage();

  useEffect(() => {
    if (logsRef.current && autoFollow) {
      logsRef.current.scrollTop = logsRef.current.scrollHeight;
    }
  }, [logs, autoFollow]);

  useEffect(() => {
    let stopped = false
    let reconnectTimer = null
    let controller = null

    const connect = async () => {
      controller = new AbortController()
      setStreamState('connecting')
      try {
        const response = await terminalStreamRequest(controller.signal)
        const contentType = response.headers.get('content-type') || ''
        if (!response.ok || !contentType.includes('text/event-stream')) {
          throw new Error(`日志流响应异常: HTTP ${response.status}`)
        }

        setLogs([])
        setStreamState('live')
        const reader = response.body.getReader()
        const decoder = new TextDecoder()
        let buffer = ''

        while (!stopped) {
          const {done, value} = await reader.read()
          if (done) break
          buffer += decoder.decode(value, {stream: true})
          const blocks = buffer.split(/\r?\n\r?\n/)
          buffer = blocks.pop() || ''
          const entries = []
          blocks.forEach(block => {
            const payload = block.split(/\r?\n/)
              .filter(line => line.startsWith('data:'))
              .map(line => line.slice(5).trimStart())
              .join('\n')
            if (!payload) return
            try {
              entries.push(JSON.parse(payload))
            } catch {
              // Ignore malformed stream frames; the next valid frame remains usable.
            }
          })
          if (entries.length > 0) {
            setLogs(current => current.concat(entries).slice(-1000))
          }
        }
        if (!stopped) throw new Error('日志流已断开')
      } catch (error) {
        if (stopped || error.name === 'AbortError') return
        setStreamState('reconnecting')
        reconnectTimer = setTimeout(connect, 1500)
      }
    }

    connect()
    return () => {
      stopped = true
      controller?.abort()
      if (reconnectTimer !== null) clearTimeout(reconnectTimer)
    }
  }, [streamEpoch])

  useEffect(() => {
    miscVersionList()
    getDiskInfo()
  }, []);

  useEffect(() => {
    localStorage.setItem('filePath', JSON.stringify(path))
    getFileList()
  }, [path])

  const getDiskInfo = async () => {
    const {code, data} = await fsDiskInfoRequest()
    if (code === 1) setDiskInfo(data)
  }

  const getFileList = async () => {
    const {code, data} = await fsListRequest(path.join('/'))
    if (code === 1) {
      const sorted = [...data.files].sort((a, b) => {
        if (a.is_directory !== b.is_directory) return a.is_directory ? -1 : 1
        return a.name.localeCompare(b.name, 'zh-CN')
      })
      setFileList(sorted)
    }
  }

  const handlerNextPath = item => setPath(current => [...current, item.name])
  const handlerBreadcrumb = index => setPath(current => current.slice(0, index))

  const changeViewMode = value => {
    setViewMode(value)
    localStorage.setItem('fileViewMode', value)
  }

  const toggleLogs = () => {
    setLogCollapsed(current => {
      localStorage.setItem('logCollapsed', (!current).toString())
      return !current
    })
  }

  const handleLogScroll = () => {
    const element = logsRef.current
    if (!element) return
    setAutoFollow(element.scrollHeight - element.scrollTop - element.clientHeight < 28)
  }

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

  const returnToPackEditor = () => {
    setPackPreview(null)
    setExcludedChangeIds([])
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
      <div className="flex h-screen min-h-[720px] flex-col overflow-hidden bg-[#f4f7f6] p-6 dark:bg-[#101615]">
        <div className="flex flex-wrap items-center gap-2">
          <VersionList versionList={versionList}/>
          <Popconfirm
            title="校验全部历史更新包？"
            description="依次回放 public 中的更新索引，核对每个归档切片能否正确读取。此操作只校验，不修改工作空间。"
            onConfirm={taskTest} okText="开始校验" cancelText="取消">
            <Button size="large">校验全部更新包</Button>
          </Popconfirm>
          <Popconfirm
            title="同步 public 到下载源？"
            description="把 public 顶层文件同步到已启用的 WebDAV/S3；会覆盖变化文件，并删除远端清单中本地已不存在的受管文件。"
            onConfirm={taskUpload} okText="开始同步" cancelText="取消">
            <Button size="large">同步 public 到下载源</Button>
          </Popconfirm>
          <Button type="primary" size="large" onClick={() => {
            const nextVersion = nextVersionForHistory(versionList.map(({label}) => label))
            if (nextVersion === '') {
              messageApi.error('无法从最新版本生成下一个版本号。')
              return
            }
            setVersion(nextVersion)
            setPackShow(true)
          }}>打包新版本</Button>
          <Popconfirm
            title="按更新历史回退工作空间？"
            description="用 public 中的完整更新历史重建 workspace。所有尚未打包的新增、删除、改名和内容修改都会被撤销。"
            onConfirm={taskRevert} okText="确认回退" cancelText="取消">
            <Button size="large" danger>回退工作空间</Button>
          </Popconfirm>
          <Popconfirm
            title="合并历史更新包？"
            description="先完整校验历史包，再将多个增量归档合并为 combined.tar；版本记录保留，原增量 tar 会从 public 删除。"
            onConfirm={taskCombine} okText="开始合并" cancelText="取消">
            <Button size="large">合并更新包</Button>
          </Popconfirm>
        </div>

        <div className="mt-5 grid grid-cols-4 border-y border-[#dce7e4] bg-[#fbfdfc] py-3 dark:border-[#263532] dark:bg-[#141d1b]">
          <div className="border-r border-gray-200 px-4 dark:border-gray-800">
            <div className="text-xs text-gray-500">当前版本</div>
            <div className="mt-1 text-lg font-semibold text-gray-800 dark:text-gray-100">{versionList[0]?.label || '-'}</div>
          </div>
          <div className="border-r border-gray-200 px-4 dark:border-gray-800">
            <div className="text-xs text-gray-500">更新包占用</div>
            <div className="mt-1 text-lg font-semibold text-gray-800 dark:text-gray-100">{showFileSize(diskInfo.public_used)}</div>
            <div className="text-xs text-gray-400">{diskInfo.public_files} 个文件</div>
          </div>
          <div className="border-r border-gray-200 px-4 dark:border-gray-800">
            <div className="text-xs text-gray-500">工作目录</div>
            <div className="mt-1 text-lg font-semibold text-gray-800 dark:text-gray-100">{showFileSize(diskInfo.workspace_used)}</div>
            <div className="text-xs text-gray-400">{diskInfo.workspace_files} 个文件</div>
          </div>
          <div className="px-4">
            <div className="text-xs text-gray-500">磁盘使用</div>
            <div className="mt-1 flex items-center gap-3">
              <Progress
                className="min-w-0 flex-1"
                percent={diskInfo.total > 0 ? Number((diskInfo.used / diskInfo.total * 100).toFixed(1)) : 0}
                size="small"
                strokeColor="#0f766e"
                trailColor="#e5e7eb"/>
            </div>
            <div className="text-xs text-gray-400">{showFileSize(diskInfo.used)} / {showFileSize(diskInfo.total)}</div>
          </div>
        </div>

        <div className="mt-4 flex min-h-0 flex-1 gap-4">
          <main className="flex min-w-0 flex-1 flex-col overflow-hidden border border-[#dce7e4] bg-white dark:border-[#263532] dark:bg-[#141d1b]">
            <div className="flex items-end justify-between gap-4 border-b border-gray-200 px-4 py-3 dark:border-gray-800">
              <FileBreadcrumb path={path} handlerBreadcrumb={handlerBreadcrumb} workspacePath={diskInfo.workspace_path}/>
              <Segmented
                className="file-view-switch"
                value={viewMode}
                onChange={changeViewMode}
                options={[
                  {value: 'grid', icon: <Tooltip title="图标视图"><span className="file-view-switch-icon"><Grid2X2 size={17}/></span></Tooltip>},
                  {value: 'list', icon: <Tooltip title="列表视图"><span className="file-view-switch-icon"><List size={18}/></span></Tooltip>}
                ]}/>
            </div>
            <div className="border-b border-gray-200 px-4 py-3 dark:border-gray-800">
              <FolderButtonGroup path={path} getFileList={() => { getFileList(); getDiskInfo() }}/>
            </div>
            <div className="min-h-0 flex-1 overflow-auto bg-[#f8fbfa] dark:bg-[#111917]">
              <TileViewFileExplorer
                path={path}
                getFileList={() => { getFileList(); getDiskInfo() }}
                items={fileList}
                handlerNextPath={handlerNextPath}
                viewMode={viewMode}/>
            </div>
          </main>

          <aside className={`${logCollapsed ? 'w-12' : 'w-[34%] min-w-[360px] max-w-[680px]'} flex shrink-0 flex-col overflow-hidden border border-gray-200 bg-gray-950 transition-[width] duration-200 dark:border-gray-800`}>
            <div className={`flex h-12 shrink-0 items-center border-b border-gray-800 ${logCollapsed ? 'justify-center px-1' : 'gap-2 px-3'}`}>
              {!logCollapsed && (
                <>
                  <div className={`h-2 w-2 rounded-full ${streamState === 'live' ? 'bg-emerald-500' : 'bg-amber-500'}`}/>
                  <span className="text-sm font-medium text-gray-100">实时日志</span>
                  <span className="text-xs text-gray-500">{streamState === 'live' ? '已连接' : streamState === 'connecting' ? '连接中' : '正在重连'}</span>
                  {!autoFollow && <span className="ml-auto text-xs text-amber-400">已暂停跟随</span>}
                  <Tooltip title="重新连接">
                    <Button type="text" className={`${autoFollow ? 'ml-auto' : ''} text-gray-300`} icon={<RotateCcw size={17}/>} onClick={() => setStreamEpoch(value => value + 1)}/>
                  </Tooltip>
                </>
              )}
              <Tooltip title={logCollapsed ? '展开日志' : '折叠日志'}>
                <Button type="text" className="text-gray-300" icon={logCollapsed ? <PanelRightOpen size={18}/> : <PanelRightClose size={18}/>} onClick={toggleLogs}/>
              </Tooltip>
            </div>
            {!logCollapsed && (
              <div ref={logsRef} onScroll={handleLogScroll} className="min-h-0 flex-1 overflow-auto px-2 py-2 font-mono text-xs leading-5 text-gray-300">
                {logs.map((item, index) => (
                  <div
                    key={`${item.time}-${index}`}
                    onClick={() => copy(item)}
                    className="grid cursor-pointer grid-cols-[132px_minmax(0,1fr)] gap-2 px-1 py-0.5 hover:bg-gray-900">
                    <span className="text-gray-600">{showTime(item.time)}</span>
                    <span className={`${getTextColor(item.level)} break-words`}>{item.content}</span>
                  </div>
                ))}
                {logs.length === 0 && <div className="py-10 text-center text-gray-600">等待日志输出</div>}
              </div>
            )}
          </aside>
        </div>
      </div>
      <Modal
        title={packPreview === null ? "打包新版本" : "确认本次文件变化"}
        width={780}
        okText={packPreview === null ? "查看变化" : "确认并打包"}
        cancelButtonProps={{style: {display: "none"}}}
        footer={(_, {OkBtn}) => (
          <div className="flex justify-end gap-2">
            {packPreview !== null && (
              <Button icon={<ArrowLeft size={17}/>} onClick={returnToPackEditor}>返回编辑</Button>
            )}
            <OkBtn/>
          </div>
        )}
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
            <div className="mb-3 flex items-center justify-between gap-3 text-sm text-gray-500">
              <div className="flex min-w-0 items-center gap-2">
                <span className="truncate">共 {packPreview.changes.length} 项，已排除 {excludedChangeIds.length} 项</span>
              </div>
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
