package mcpatch;

import java.io.BufferedReader;
import java.io.File;
import java.io.IOException;
import java.io.InputStreamReader;
import java.lang.instrument.Instrumentation;
import java.net.URL;
import java.net.URLDecoder;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.awt.Desktop;
import java.awt.GraphicsEnvironment;
import javax.swing.JDialog;
import javax.swing.JOptionPane;
import javax.swing.JTextArea;
import javax.swing.UIManager;

public class Mcpatch2Loader {
    private static final int UPDATE_BLOCKED_EXIT_CODE = 10;
    private static final String CLIENT_SOURCE_URL = "https://github.com/BalloonUpdate/Mcpatch2RustClient";
    public static void main(String[] args) throws IOException, InterruptedException {
        entrance();
    }

    public static void premain(String args, Instrumentation ins) throws IOException, InterruptedException {
        entrance();
    }

    static void entrance() throws IOException, InterruptedException {
        // 获取自己Jar文件位置
        File jarFile = getJarPath();

        if (jarFile == null)
            throw new RuntimeException("failed to get the path of self jar-file");

        // 读取启动列表文件
        File startListFile = new File(jarFile.getParentFile(), "startlist.txt");
        String startListPath = startListFile.getAbsolutePath();

        // 创建一个空的启动列表文件
        if (!startListFile.exists()) {
            try {
                //noinspection ResultOfMethodCallIgnored
                startListFile.createNewFile();
            } catch (IOException e) {
                throw new RuntimeException("failed to create the file: " + startListPath, e);
            }
        }

        // 加载启动列表文件
        List<String> content;

        try {
            content = Files.readAllLines(startListFile.toPath());
        } catch (IOException e) {
            throw new RuntimeException("failed to read the file: " + startListPath, e);
        }

        List<File> candidates = executableCandidates(content, startListPath, jarFile);
        Process process = null;
        File exeFile = null;
        List<String> launchFailures = new ArrayList<>();
        boolean possibleSecuritySoftwareInterference = false;

        // A newly downloaded updater may be missing or unusable after an
        // interrupted update. Try the ordered fallbacks without deleting any
        // known-good executable first.
        for (File candidate : candidates) {
            if (!candidate.isFile())
                continue;

            try {
                ProcessBuilder pb = new ProcessBuilder(candidate.getAbsolutePath());
                pb.redirectErrorStream(true);
                process = pb.start();
                exeFile = candidate;
                break;
            } catch (IOException e) {
                String failure = candidate.getName() + ": " + e.getMessage();
                launchFailures.add(failure);
                possibleSecuritySoftwareInterference |= isPossibleSecuritySoftwareInterference(e);
                System.err.println("failed to start mcpatch candidate " + failure);
            }
        }

        if (process == null || exeFile == null) {
            if (possibleSecuritySoftwareInterference)
                showSecuritySoftwareHint(candidates.get(0), launchFailures);
            throw new RuntimeException("no startable updater executable found in: " + startListPath
                    + "; failures: " + String.join(" | ", launchFailures));
        }

        System.out.println("mcpatch-executable is " + exeFile.getAbsolutePath());
        Process launchedProcess = process;

        // 捕获stdout并解码
        new Thread(() -> {
            InputStreamReader reader = new InputStreamReader(launchedProcess.getInputStream(), StandardCharsets.UTF_8);
            BufferedReader input = new BufferedReader(reader);

            while (true) {
                String line;

                try {
                    line = input.readLine();
                } catch (IOException e) {
                    throw new RuntimeException(e);
                }

                if (line == null)
                    break;

                System.out.println(line);
            }
        }).start();

        // 等待mcpatch退出
        int exitCode = process.waitFor();

        System.out.println("mcpatch returns: " + exitCode);

        // EXE has already shown an actionable file-operation dialog. End the
        // launch cleanly instead of converting this expected user-facing stop
        // into a Java Agent/FML exception.
        if (exitCode == UPDATE_BLOCKED_EXIT_CODE) {
            System.exit(exitCode);
        }

        // Other nonzero exit values still represent an updater fault.
        if (exitCode != 0) {
            throw new RuntimeException("DLL returns " + exitCode + " as exitcode, it's not 0 as expected.");
        }

        // Only retire older updater binaries after the selected executable has
        // completed successfully. This preserves rollback on launch/update
        // failure while still cleaning side-by-side self-update artifacts.
        removeOtherExecutables(candidates, exeFile);
    }

    /**
     * 获取exe文件的路径，同时删除其它旧的文件
     * @param content 启动列表文件的内容
     * @param startListPath 启动列表文件路径
     * @param jarFile 自己Jar文件
     */
    static List<File> executableCandidates(List<String> content, String startListPath, File jarFile) {
        if (content.isEmpty()) {
            throw new RuntimeException("the file can not be empty: " + startListPath);
        }

        List<File> candidates = new ArrayList<>();
        File parent = jarFile.getParentFile();
        for (String line : content) {
            String name = line.trim();

            if (name.isEmpty())
                continue;

            // startlist is an executable rotation list, not a general-purpose
            // path deletion mechanism. Keep every entry inside Loader.jar's
            // directory and accept Windows executables only.
            if (new File(name).isAbsolute()
                    || name.contains("/")
                    || name.contains("\\")
                    || !name.toLowerCase(Locale.ROOT).endsWith(".exe")) {
                System.err.println("ignored unsafe mcpatch startlist entry: " + name);
                continue;
            }

            File candidate = new File(parent, name);
            if (!candidates.contains(candidate))
                candidates.add(candidate);
        }

        if (candidates.isEmpty())
            throw new RuntimeException("no safe updater executable found in: " + startListPath);

        return candidates;
    }

    static void removeOtherExecutables(List<File> candidates, File selected) {
        for (File candidate : candidates) {
            if (candidate.equals(selected) || !candidate.exists())
                continue;

            try {
                Files.delete(candidate.toPath());
            } catch (IOException e) {
                System.err.println("failed to remove old mcpatch executable " + candidate.getName() + ": " + e.getMessage());
            }
        }
    }

    static boolean isPossibleSecuritySoftwareInterference(IOException error) {
        String message = String.valueOf(error.getMessage()).toLowerCase(Locale.ROOT);
        return message.contains("createprocess error=14001");
    }

    static String securitySoftwareHint(File updater, List<String> failures) {
        return "自动更新器无法启动，可能被杀毒软件拦截、隔离，或正在接受延迟扫描。\n\n"
                + "请在杀毒软件中恢复并允许这个文件，然后重新启动客户端：\n"
                + updater.getAbsolutePath() + "\n\n"
                + "MCUpdate 代码已在 GitHub 开源，可以审查代码后放心允许：\n"
                + CLIENT_SOURCE_URL + "\n\n"
                + "详细错误：\n" + String.join("\n", failures);
    }

    private static void showSecuritySoftwareHint(File updater, List<String> failures) {
        String message = securitySoftwareHint(updater, failures);
        if (GraphicsEnvironment.isHeadless()) {
            System.err.println(message);
            return;
        }

        JTextArea text = new JTextArea(message, 12, 56);
        text.setEditable(false);
        text.setLineWrap(true);
        text.setWrapStyleWord(true);
        text.setOpaque(false);
        text.setFont(UIManager.getFont("Label.font"));

        Object openSource = "查看开源代码";
        Object close = "关闭";
        JOptionPane pane = new JOptionPane(
                text,
                JOptionPane.ERROR_MESSAGE,
                JOptionPane.DEFAULT_OPTION,
                null,
                new Object[] { openSource, close },
                openSource);
        JDialog dialog = pane.createDialog("自动更新器启动失败");
        dialog.setAlwaysOnTop(true);
        dialog.setVisible(true);
        dialog.dispose();

        if (openSource.equals(pane.getValue()) && Desktop.isDesktopSupported()) {
            try {
                Desktop.getDesktop().browse(URI.create(CLIENT_SOURCE_URL));
            } catch (Exception error) {
                System.err.println("failed to open MCUpdate source URL: " + error.getMessage());
            }
        }
    }

    /**
     * 获取自己Jar的路径
     */
    static File getJarPath() {
        URL resource = Mcpatch2Loader.class.getResource("");

        if (resource != null && resource.getProtocol().equals("file"))
            return null;

        try {
            URL location = Mcpatch2Loader.class.getProtectionDomain().getCodeSource().getLocation();
            String url = URLDecoder.decode(location.getPath(), "UTF-8").replace("\\", "/");

            if (url.endsWith(".class") && url.contains("!")) {
                String path = url.substring(0, url.lastIndexOf("!"));

                if (path.contains("file:/"))
                    path = path.substring(path.indexOf("file:/") + "file:/".length());

                return new File(path);
            } else {
                return new File(url);
            }
        } catch (Exception e) {
            throw new RuntimeException("Failed to decode or process URL", e);
        }
    }
}
