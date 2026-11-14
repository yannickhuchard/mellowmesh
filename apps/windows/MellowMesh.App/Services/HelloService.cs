using System.Reflection;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;
using Windows.Security.Credentials.UI;

namespace MellowMesh.App.Services;

/// <summary>
/// "Humans decide, helpers act": every answer to a request is confirmed with
/// Windows Hello (face, fingerprint or PIN). Without Hello the app asks with an
/// explicit dialog instead. Nothing here is reachable from MeshUI cards.
/// </summary>
public static class HelloService
{
    public static async Task<bool> ConfirmAsync(XamlRoot root, string title, string message)
    {
        try
        {
            var availability = await UserConsentVerifier.CheckAvailabilityAsync();
            if (availability == UserConsentVerifierAvailability.Available)
            {
                var result = await RequestForWindowAsync(message) ?? await UserConsentVerifier.RequestVerificationAsync(message);
                return result == UserConsentVerificationResult.Verified;
            }
        }
        catch (Exception)
        {
            // Hello could not be shown from this window: fall back to the dialog.
        }
        return await ConfirmWithDialogAsync(root, title, message);
    }

    /// <summary>
    /// Uses <c>UserConsentVerifierInterop.RequestVerificationForWindowAsync</c>
    /// (so the Hello prompt is parented to our window) when the projection has it.
    /// </summary>
    private static async Task<UserConsentVerificationResult?> RequestForWindowAsync(string message)
    {
        var window = App.Shell;
        if (window is null)
        {
            return null;
        }
        try
        {
            var interop = typeof(UserConsentVerifier).Assembly.GetType("Windows.Security.Credentials.UI.UserConsentVerifierInterop");
            var method = interop?.GetMethod("RequestVerificationForWindowAsync", BindingFlags.Public | BindingFlags.Static);
            if (method is null)
            {
                return null;
            }
            var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(window);
            if (method.Invoke(null, new object[] { hwnd, message }) is IAsyncOperation<UserConsentVerificationResult> op)
            {
                return await op;
            }
        }
        catch (Exception)
        {
        }
        return null;
    }

    public static async Task<bool> ConfirmWithDialogAsync(XamlRoot root, string title, string message)
    {
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = title,
            Content = new TextBlock { Text = message, TextWrapping = TextWrapping.Wrap },
            PrimaryButtonText = "Yes, send my answer",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        return await dialog.ShowAsync() == ContentDialogResult.Primary;
    }
}
