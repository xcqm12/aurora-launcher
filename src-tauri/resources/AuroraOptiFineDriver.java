import java.io.File;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;

/** Calls optifine.Installer.doInstall(File) for a chosen game directory.
 *  The installer's own main() hard-codes the default .minecraft folder. */
public class AuroraOptiFineDriver {
    public static void main(String[] args) throws Exception {
        if (args.length < 1) {
            System.err.println("usage: AuroraOptiFineDriver <minecraft dir>");
            System.exit(2);
        }
        File dir = new File(args[0]).getCanonicalFile();
        Class<?> installer = Class.forName("optifine.Installer");
        Constructor<?> ctor = installer.getDeclaredConstructor();
        ctor.setAccessible(true);
        Object instance = ctor.newInstance();
        Method doInstall = installer.getDeclaredMethod("doInstall", File.class);
        doInstall.setAccessible(true);
        doInstall.invoke(instance, dir);
    }
}
