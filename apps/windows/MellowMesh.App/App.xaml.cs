using MellowMesh.App.Services;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;
using Microsoft.Windows.AppNotifications;

namespace MellowMesh.App;

/// <summary>Application entry: one main window, single instance, notification activation.</summary>
public partial class App : Application
{
    private const string InstanceKey = "MellowMesh.Main";

    public App()
    {
        InitializeComponent();
        UnhandledException += OnUnhandledException;
    }

    public static MainWindow? Shell { get; private set; }

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        // Single instance: a second launch (e.g. a click on a notification while
        // MellowMesh runs) is handed to the running window.
        var activated = AppInstance.GetCurrent().GetActivatedEventArgs();
        var main = AppInstance.FindOrRegisterForKey(InstanceKey);
        if (!main.IsCurrent)
        {
            try
            {
                await main.RedirectActivationToAsync(activated);
            }
            catch
            {
                // Fall through and exit anyway.
            }
            Exit();
            return;
        }
        main.Activated += OnRedirectedActivation;

        NotificationService.Instance.Initialize();

        Shell = new MainWindow();
        Shell.Activate();

        if (activated.Kind == ExtendedActivationKind.AppNotification
            && activated.Data is AppNotificationActivatedEventArgs toast)
        {
            NotificationService.Instance.QueueActivation(toast.Arguments);
        }
        await Shell.StartAsync();
    }

    private void OnRedirectedActivation(object? sender, AppActivationArguments e)
    {
        var window = Shell;
        if (window is null)
        {
            return;
        }
        window.DispatcherQueue.TryEnqueue(() =>
        {
            window.BringToFront();
            if (e.Kind == ExtendedActivationKind.AppNotification && e.Data is AppNotificationActivatedEventArgs toast)
            {
                NotificationService.Instance.HandleActivation(toast.Arguments);
            }
        });
    }

    private void OnUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs e)
    {
        // Keep the app alive and tell the user plainly; never log secrets.
        e.Handled = true;
        Shell?.ShowWhisper("Something went wrong: " + MellowMesh.Core.SafeText.Preview(e.Message, 160), isError: true);
    }
}
