import React, {useEffect, useRef, useState} from 'react';
import FileItem from "@/components/TileViewFileExplorer/FileItem/index.jsx";
import './index.css'
import {fsDeleteRequest, fsSignFileRequest} from "@/api/fs.js";
import {Button, message, Modal} from "antd";
import {showFileSize, showTime} from "@/utils/tool.js";
import {FileText, Folder} from "lucide-react";

const Index = ({path, getFileList, items, handlerNextPath, viewMode = 'grid'}) => {

  const [isOpen, setIsOpen] = useState(false);
  const [menuPosition, setMenuPosition] = useState({x: 0, y: 0});
  const [isAnimating, setIsAnimating] = useState(false);
  const [selectedItem, setSelectedItem] = useState({})
  const [preview, setPreview] = useState(null)
  const [previewLoading, setPreviewLoading] = useState(false)
  const menuRef = useRef(null);
  const [messageApi, contextHolder] = message.useMessage();

  const handleContextMenu = (e, index) => {
    e.preventDefault();
    const {clientX: mouseX, clientY: mouseY} = e;
    setSelectedItem(items[index]);
    setMenuPosition({x: mouseX, y: mouseY});
    setIsOpen(true);
    setIsAnimating(false)
    setTimeout(() => setIsAnimating(true), 5);
  };

  const closeMenu = () => setIsOpen(false);

  useEffect(() => {
    const handleClickOutside = (e) => {
      if (menuRef.current && !menuRef.current.contains(e.target)) {
        closeMenu();
      }
    };

    document.addEventListener('click', handleClickOutside);

    return () => {
      document.removeEventListener('click', handleClickOutside);
    };
  }, []);

  const signedFileUrl = async (item, previewMode = false) => {
    let key = path.join('/');
    key = key.length === 0 ? item.name : `${key}/${item.name}`
    const {code, msg, data} = await fsSignFileRequest(key);
    if (code !== 1) throw new Error(msg)
    return `${import.meta.env.VITE_API_URL}/fs/extract-file?sign=${encodeURIComponent(data.signature)}${previewMode ? '&preview=1' : ''}`
  }

  const fsOpen = async (item) => {
    closeMenu()
    if (item.is_directory) {
      handlerNextPath(item)
      return
    }
    setPreviewLoading(true)
    try {
      setPreview({item, url: await signedFileUrl(item, true)})
    } catch (error) {
      messageApi.error(error.message || '无法打开文件预览')
    } finally {
      setPreviewLoading(false)
    }
  }

  const fsDownload = async (item) => {
    closeMenu()
    try {
      const link = document.createElement('a');
      link.href = await signedFileUrl(item);
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
    } catch (error) {
      messageApi.error(error.message || '无法下载文件')
    }
  }

  const previewKind = (name = '') => {
    const extension = name.split('.').pop()?.toLowerCase()
    if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'].includes(extension)) return 'image'
    if (['txt', 'log', 'md', 'toml', 'yml', 'yaml', 'properties', 'json', 'pdf'].includes(extension)) return 'frame'
    if (['mp3', 'ogg', 'wav'].includes(extension)) return 'audio'
    if (['mp4', 'webm'].includes(extension)) return 'video'
    return 'unsupported'
  }

  const fsDelete = async (item) => {
    let key = path.join('/');
    key = key.length === 0 ? item.name : `${key}/${item.name}`

    const {code, msg, data} = await fsDeleteRequest(key);
    if (code === 1) {
      messageApi.success('删除成功')
      getFileList()
    } else {
      messageApi.error(msg);
    }
    closeMenu()
  }

  const statusText = (state) => ({
    keep: '未变化',
    added: '新增',
    modified: '已修改',
    missing: '缺失',
    gone: '已移除',
    come: '已恢复'
  }[state] || state)

  const statusColor = (state) => ({
    added: 'text-green-600',
    modified: 'text-amber-600',
    missing: 'text-red-600',
    gone: 'text-cyan-600',
    come: 'text-violet-600'
  }[state] || 'text-gray-500')

  return (
    <>
      {contextHolder}
      {viewMode === 'grid' ? (
        <div className="flex flex-wrap content-start">
          {items.map((item, index) => (
            <div
              key={item.name}
              onDoubleClick={() => fsOpen(item)}
              onContextMenu={(e) => handleContextMenu(e, index)} onClick={closeMenu}>
              <FileItem item={item}/>
            </div>
          ))}
        </div>
      ) : (
        <div className="min-w-[680px] select-none text-sm">
          <div className="grid h-10 grid-cols-[minmax(240px,1fr)_110px_110px_180px] items-center border-b border-gray-200 px-3 font-medium text-gray-500 dark:border-gray-700">
            <span>名称</span><span>状态</span><span>大小</span><span>修改时间</span>
          </div>
          {items.map((item, index) => (
            <div
              key={item.name}
              className="grid h-11 cursor-pointer grid-cols-[minmax(240px,1fr)_110px_110px_180px] items-center border-b border-[#e7efed] px-3 text-gray-700 hover:bg-teal-50/70 dark:border-[#263532] dark:text-gray-200 dark:hover:bg-teal-950/30"
              onDoubleClick={() => fsOpen(item)}
              onContextMenu={(e) => handleContextMenu(e, index)}
              onClick={closeMenu}>
              <div className="flex min-w-0 items-center gap-2">
                {item.is_directory ? <Folder size={18} className="shrink-0 text-teal-600 dark:text-teal-400"/> : <FileText size={18} className="shrink-0 text-gray-400"/>}
                <span className="truncate" title={item.name}>{item.name}</span>
              </div>
              <span className={statusColor(item.state)}>{statusText(item.state)}</span>
              <span>{item.is_directory ? '-' : showFileSize(item.size)}</span>
              <span>{showTime(item.mtime)}</span>
            </div>
          ))}
        </div>
      )}

      {
        isOpen ?
          <div
            ref={menuRef}
            className="absolute bg-white dark:bg-gray-900 rounded-md shadow-lg w-60"
            style={{
              left: `${menuPosition.x}px`,
              top: `${menuPosition.y}px`,
              opacity: isAnimating ? 1 : 0,
              transform: isAnimating ? 'scale(1)' : 'scale(0.9)',
              transition: 'opacity 0.2s ease, transform 0.2s ease',
            }}
          >
            <div className="p-1">
              <div
                className="flex flex-col rounded-md w-full p-2 text-sm cursor-pointer hover:bg-gray-200 dark:hover:bg-gray-800 duration-200">
                <div className="text-item">名称: {selectedItem.name}</div>
                <div className="text-item">类型: {selectedItem.is_directory ? "文件夹" : "文件"}</div>
                <div className="text-item">大小: {showFileSize(selectedItem.size)}</div>
                <div className="text-item">状态: {selectedItem.state}</div>
                <div className="text-item">创建时间: {showTime(selectedItem.ctime)}</div>
                <div className="text-item">修改时间: {showTime(selectedItem.mtime)}</div>
              </div>
              {
                selectedItem.is_directory &&
                <>
                  <button
                    onClick={() => fsOpen(selectedItem)}
                    className="flex w-full items-center rounded-md p-2 text-sm text-teal-700 duration-200 hover:bg-teal-50 dark:text-teal-300 dark:hover:bg-teal-950/30">
                    打开
                  </button>
                </>
              }
              {
                !selectedItem.is_directory &&
                <>
                  <button
                    onClick={() => fsOpen(selectedItem)}
                    className="flex w-full items-center rounded-md p-2 text-sm text-teal-700 duration-200 hover:bg-teal-50 dark:text-teal-300 dark:hover:bg-teal-950/30">
                    预览
                  </button>
                  <button
                    onClick={() => fsDownload(selectedItem)}
                    className="flex w-full items-center rounded-md p-2 text-sm text-teal-700 duration-200 hover:bg-teal-50 dark:text-teal-300 dark:hover:bg-teal-950/30">
                    下载
                  </button>
                </>
              }
              {
                <button
                  onClick={() => fsDelete(selectedItem)}
                  className="flex items-center rounded-md w-full p-2 text-sm text-red-500 hover:bg-red-100 dark:hover:bg-gray-800 duration-200">
                  删除
                </button>
              }
            </div>
          </div> : <></>
      }
      <Modal
        title={preview?.item?.name || '文件预览'}
        open={Boolean(preview) || previewLoading}
        width="min(1000px, 88vw)"
        centered
        loading={previewLoading}
        onCancel={() => setPreview(null)}
        footer={preview ? [
          <Button key="download" onClick={() => fsDownload(preview.item)}>下载</Button>,
          <Button key="close" type="primary" onClick={() => setPreview(null)}>关闭</Button>
        ] : null}>
        {preview && previewKind(preview.item.name) === 'image' && <div className="grid min-h-80 place-items-center rounded-md bg-gray-100 p-4 dark:bg-gray-950"><img src={preview.url} alt={preview.item.name} className="max-h-[65vh] max-w-full object-contain"/></div>}
        {preview && previewKind(preview.item.name) === 'frame' && <iframe title={preview.item.name} src={preview.url} className="h-[65vh] w-full rounded-md border border-gray-200 bg-white dark:border-gray-700"/>}
        {preview && previewKind(preview.item.name) === 'audio' && <div className="grid min-h-48 place-items-center"><audio controls src={preview.url} className="w-full"/></div>}
        {preview && previewKind(preview.item.name) === 'video' && <video controls src={preview.url} className="max-h-[65vh] w-full rounded-md bg-black"/>}
        {preview && previewKind(preview.item.name) === 'unsupported' && <div className="grid min-h-48 place-items-center rounded-md border border-dashed border-gray-300 text-sm text-gray-500 dark:border-gray-700 dark:text-gray-400">此文件类型无法在线预览，可使用下方按钮下载。</div>}
      </Modal>
    </>
  );
};

export default Index;
