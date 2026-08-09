using System.Text;

namespace AiWakeScheduler.Core;

/// <summary>
/// 提供命令列參數字串的解析與 Token 化處理。
/// </summary>
public static class ArgumentTokenizer
{
    /// <summary>
    /// 解析命令列參數字串，處理雙引號與斜線轉義。
    /// </summary>
    /// <param name="commandLine">傳入的命令列參數字串</param>
    /// <returns>解析後的參數清單</returns>
    /// <exception cref="FormatException">當雙引號未正常閉合時擲出</exception>
    public static IReadOnlyList<string> Parse(string? commandLine)
    {
        if (string.IsNullOrWhiteSpace(commandLine))
        {
            return Array.Empty<string>();
        }

        var arguments = new List<string>();
        var current = new StringBuilder(commandLine.Length);
        var inQuotes = false;
        var backslashes = 0;

        foreach (var character in commandLine)
        {
            if (character == '\\')
            {
                backslashes++;
                continue;
            }

            if (character == '"')
            {
                current.Append('\\', backslashes / 2);
                if (backslashes % 2 == 1)
                {
                    current.Append('"');
                }
                else
                {
                    inQuotes = !inQuotes;
                }

                backslashes = 0;
                continue;
            }

            current.Append('\\', backslashes);
            backslashes = 0;

            if (char.IsWhiteSpace(character) && !inQuotes)
            {
                if (current.Length > 0)
                {
                    arguments.Add(current.ToString());
                    current.Clear();
                }
            }
            else
            {
                current.Append(character);
            }
        }

        current.Append('\\', backslashes);
        if (inQuotes)
        {
            throw new FormatException("額外參數含有未結束的雙引號。");
        }

        if (current.Length > 0)
        {
            arguments.Add(current.ToString());
        }

        return arguments;
    }
}


