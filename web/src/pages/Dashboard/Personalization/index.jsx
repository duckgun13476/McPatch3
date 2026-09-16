import React, {useEffect, useMemo, useState} from "react";
import {Button, ColorPicker, Input, InputNumber, message, Upload} from "antd";
import {Image, RotateCcw, Save, UploadCloud, X} from "lucide-react";
import {
  personalizationGetRequest,
  personalizationRemoveImageRequest,
  personalizationSaveRequest,
  personalizationUploadImageRequest
} from "@/api/personalization.js";

const themeFields = [
  ['accent', '主色'], ['accentHover', '悬浮色'], ['accentSoft', '浅强调色'],
  ['background', '窗口底色'], ['surface', '内容面板'], ['logBackground', '日志底色'],
  ['text', '主要文字'], ['muted', '次要文字'], ['border', '边框']
]

const apiBase = import.meta.env.VITE_API_URL || '/api'
const serverBase = apiBase.endsWith('/api') ? apiBase.slice(0, -4) : ''
const publicAsset = (path, revision) => path ? `${serverBase}/public/${path}?v=${revision}` : ''

const ImageField = ({title, hint, preview, onSelect, onRemove}) => (
  <div className="space-y-2">
    <div>
      <div className="text-sm font-semibold text-[#243936] dark:text-[#e5efec]">{title}</div>
      <div className="mt-1 text-xs text-[#6b7f7b] dark:text-[#91a7a1]">{hint}</div>
    </div>
    <div className="flex items-center gap-3">
      <div className="grid h-16 w-24 place-items-center overflow-hidden rounded-md border border-[#d7e3df] bg-[#eef4f2] dark:border-[#344945] dark:bg-[#172320]">
        {preview ? <img src={preview} alt="" className="h-full w-full object-cover"/> : <Image size={22} className="text-[#82928f]"/>}
      </div>
      <Upload accept="image/png,image/gif,image/jpeg,image/webp" showUploadList={false} beforeUpload={file => { onSelect(file); return false }}>
        <Button icon={<UploadCloud size={16}/>}>选择图片</Button>
      </Upload>
      {preview && <Button aria-label={`移除${title}`} title={`移除${title}`} icon={<X size={16}/>} onClick={onRemove}/>} 
    </div>
  </div>
)

const Index = () => {
  const [messageApi, contextHolder] = message.useMessage()
  const [profile, setProfile] = useState(null)
  const [original, setOriginal] = useState(null)
  const [iconFile, setIconFile] = useState(undefined)
  const [backgroundFile, setBackgroundFile] = useState(undefined)
  const [revision, setRevision] = useState(Date.now())
  const [saving, setSaving] = useState(false)

  const load = async () => {
    const response = await personalizationGetRequest()
    if (response?.code !== 1) {
      messageApi.error(response?.msg || '读取个性化配置失败')
      return
    }
    setProfile(response.data)
    setOriginal(response.data)
    setIconFile(undefined)
    setBackgroundFile(undefined)
    setRevision(Date.now())
  }

  useEffect(() => { load() }, [])

  const iconPreview = useMemo(() => {
    if (iconFile instanceof File) return URL.createObjectURL(iconFile)
    if (iconFile === null) return ''
    return publicAsset(profile?.icon, revision)
  }, [iconFile, profile?.icon, revision])

  const backgroundPreview = useMemo(() => {
    if (backgroundFile instanceof File) return URL.createObjectURL(backgroundFile)
    if (backgroundFile === null) return ''
    return publicAsset(profile?.backgroundImage, revision)
  }, [backgroundFile, profile?.backgroundImage, revision])

  useEffect(() => () => {
    if (iconPreview.startsWith('blob:')) URL.revokeObjectURL(iconPreview)
    if (backgroundPreview.startsWith('blob:')) URL.revokeObjectURL(backgroundPreview)
  }, [iconPreview, backgroundPreview])

  const updateProfile = (key, value) => setProfile(current => ({...current, [key]: value}))
  const updateTheme = (key, value) => setProfile(current => ({...current, theme: {...current.theme, [key]: value}}))

  const save = async () => {
    setSaving(true)
    try {
      for (const [kind, file] of [['icon', iconFile], ['background', backgroundFile]]) {
        if (file instanceof File) {
          const result = await personalizationUploadImageRequest(kind, file)
          if (result?.code !== 1) throw new Error(result?.msg || `${kind} 上传失败`)
        } else if (file === null) {
          const result = await personalizationRemoveImageRequest(kind)
          if (result?.code !== 1) throw new Error(result?.msg || `${kind} 移除失败`)
        }
      }
      const result = await personalizationSaveRequest({
        headline: profile.headline,
        subtitle: profile.subtitle,
        footer: profile.footer,
        launchLabelOffsetX: profile.launchLabelOffsetX,
        theme: profile.theme
      })
      if (result?.code !== 1) throw new Error(result?.msg || '保存失败')
      setProfile(result.data)
      setOriginal(result.data)
      setIconFile(undefined)
      setBackgroundFile(undefined)
      setRevision(Date.now())
      messageApi.success('个性化配置已保存，将在更新器下次启动时生效')
    } catch (error) {
      messageApi.error(error.message || '保存个性化配置失败')
    } finally {
      setSaving(false)
    }
  }

  if (!profile) return <div className="p-10 text-sm text-[#6b7f7b] dark:text-[#91a7a1]">正在读取个性化配置...</div>

  const theme = profile.theme
  return (
    <>
      {contextHolder}
      <main className="min-h-screen bg-[#f5f8f7] p-8 text-[#243936] dark:bg-[#0f1715] dark:text-[#e5efec]">
        <div className="mx-auto max-w-[1280px]">
          <div className="mb-6 flex items-end justify-between gap-4">
            <div>
              <h1 className="m-0 text-2xl font-semibold">个性化</h1>
              <p className="mb-0 mt-2 text-sm text-[#6b7f7b] dark:text-[#91a7a1]">配置会由自动更新器在下次启动时获取并缓存。</p>
            </div>
            <div className="flex gap-2">
              <Button icon={<RotateCcw size={16}/>} onClick={() => { setProfile(original); setIconFile(undefined); setBackgroundFile(undefined) }}>撤销修改</Button>
              <Button type="primary" icon={<Save size={16}/>} loading={saving} onClick={save}>确认保存</Button>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-6 xl:grid-cols-[minmax(0,1.35fr)_minmax(360px,0.65fr)]">
            <section>
              <div className="mb-3 text-sm font-semibold">实时预览</div>
              <div className="aspect-[1.618/1] min-h-[520px] overflow-hidden rounded-xl border shadow-[0_18px_55px_rgba(27,66,58,0.14)]"
                   style={{backgroundColor: theme.background, backgroundImage: backgroundPreview ? `url(${backgroundPreview})` : 'none', backgroundPosition: 'center', backgroundSize: 'cover', borderColor: theme.border, color: theme.text}}>
                <div className="flex h-full flex-col gap-4 p-8">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-4">
                      <div className="grid h-14 w-14 place-items-center overflow-hidden rounded-lg text-sm font-extrabold text-white" style={{background: theme.accent}}>
                        {iconPreview ? <img src={iconPreview} alt="" className="h-full w-full object-cover"/> : 'UP'}
                      </div>
                      <div><div className="text-2xl font-bold">{profile.headline}</div><div className="mt-1 text-sm" style={{color: theme.muted}}>{profile.subtitle}</div></div>
                    </div>
                    <div className="rounded-full px-3 py-2 text-xs font-bold" style={{background: theme.accentSoft, color: theme.accent}}>正在下载</div>
                  </div>
                  <div className="mt-2 rounded-lg border p-6 shadow-sm" style={{background: theme.surface, borderColor: theme.border}}>
                    <div className="text-xs font-bold" style={{color: theme.muted}}>更新状态</div>
                    <div className="mt-2 flex items-end justify-between"><span className="text-xl font-bold">正在应用客户端更新</span><span className="text-2xl font-extrabold" style={{color: theme.accent}}>64%</span></div>
                    <div className="mt-2 text-sm" style={{color: theme.muted}}>正在下载所需文件</div>
                    <div className="mt-5 h-3 overflow-hidden rounded-full" style={{background: theme.border}}><div className="h-full w-[64%] rounded-full" style={{background: theme.accent}}/></div>
                  </div>
                  <div className="min-h-0 flex-1 rounded-lg border p-5" style={{background: theme.surface, borderColor: theme.border}}>
                    <div className="font-semibold">更新日志</div>
                    <div className="mt-3 h-[calc(100%-32px)] rounded-md border p-4 text-sm leading-7" style={{background: theme.logBackground, borderColor: theme.border}}>修复客户端显示问题<br/>优化自动更新体验<br/>调整资源加载流程</div>
                  </div>
                  <div className="text-center text-xs" style={{color: theme.muted}}>{profile.footer}</div>
                </div>
              </div>
            </section>

            <section className="space-y-6 rounded-lg border border-[#dce7e4] bg-white p-6 dark:border-[#2c403c] dark:bg-[#141d1b]">
              <div className="space-y-4">
                <div className="text-sm font-semibold">显示内容</div>
                <label className="block text-xs text-[#6b7f7b] dark:text-[#91a7a1]">主标题<Input className="mt-2" value={profile.headline} maxLength={80} onChange={event => updateProfile('headline', event.target.value)}/></label>
                <label className="block text-xs text-[#6b7f7b] dark:text-[#91a7a1]">副标题<Input className="mt-2" value={profile.subtitle} maxLength={80} onChange={event => updateProfile('subtitle', event.target.value)}/></label>
                <label className="block text-xs text-[#6b7f7b] dark:text-[#91a7a1]">底部提示<Input className="mt-2" value={profile.footer} maxLength={80} onChange={event => updateProfile('footer', event.target.value)}/></label>
                <label className="flex items-center justify-between text-xs text-[#6b7f7b] dark:text-[#91a7a1]">启动文字水平偏移<InputNumber min={-24} max={24} value={profile.launchLabelOffsetX} onChange={value => updateProfile('launchLabelOffsetX', value ?? 0)}/></label>
              </div>
              <div className="h-px bg-[#e5ecea] dark:bg-[#2c403c]"/>
              <ImageField title="更新器图标" hint="支持 PNG、GIF、JPEG、WebP，最大 8 MiB。" preview={iconPreview} onSelect={setIconFile} onRemove={() => setIconFile(null)}/>
              <ImageField title="更新器背景图" hint="建议使用与窗口接近黄金比例的横向图片。" preview={backgroundPreview} onSelect={setBackgroundFile} onRemove={() => setBackgroundFile(null)}/>
              <div className="h-px bg-[#e5ecea] dark:bg-[#2c403c]"/>
              <div>
                <div className="mb-3 text-sm font-semibold">配色</div>
                <div className="grid grid-cols-2 gap-x-4 gap-y-3">
                  {themeFields.map(([key, label]) => <label key={key} className="flex items-center justify-between gap-2 text-xs text-[#536965] dark:text-[#a7bbb6]"><span>{label}</span><ColorPicker disabledAlpha size="small" value={theme[key]} onChange={color => updateTheme(key, color.toHexString())}/></label>)}
                </div>
              </div>
            </section>
          </div>
        </div>
      </main>
    </>
  )
}

export default Index
