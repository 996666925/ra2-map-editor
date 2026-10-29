// Local compatibility harness: uses FinalAlert2's unmodified XCC decode80c
// (extracted into the build directory) and its bundled lzo2.dll.
#include <windows.h>
#include <cassert>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <vector>
using byte = unsigned char;
#include "fa2_decode80.inc"
using LzoDecode = int (__cdecl *)(const byte*, size_t, byte*, size_t*, void*);
static std::vector<byte> packed(const std::string& text, const std::string& section) {
    std::string payload, active, line;
    std::istringstream input(text);
    while (std::getline(input, line)) {
        auto comment=line.find(';'); if(comment!=std::string::npos)line.resize(comment);
        auto first=line.find_first_not_of(" \t\r");if(first==std::string::npos)continue;
        if(line[first]=='['){auto end=line.find(']',first);active=line.substr(first+1,end-first-1);continue;}
        if(active!=section)continue;
        auto eq=line.find('=');if(eq!=std::string::npos)payload+=line.substr(eq+1);
    }
    std::vector<byte> out;unsigned bits=0;int count=0;
    const std::string alphabet="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for(char c:payload){if(c=='=')break;if(c==' '||c=='\r'||c=='\t')continue;auto index=alphabet.find(c);if(index==std::string::npos)throw "bad base64";bits=(bits<<6)|index;count+=6;if(count>=8){count-=8;out.push_back(byte(bits>>count));}}
    return out;
}
static unsigned word(const byte* p){return p[0]|(unsigned(p[1])<<8);}
static std::vector<byte> unpack(const std::vector<byte>& src, LzoDecode lzo, bool is_lzo) {
    std::vector<byte> output;size_t offset=0;
    while(offset<src.size()){
        if(offset+4>src.size())throw "truncated chunk header";
        auto size_in=word(&src[offset]);auto size_out=word(&src[offset+2]);offset+=4;
        if(offset+size_in>src.size()||!size_in||size_out>8192)throw "invalid chunk size";
        // Spare space makes the original unchecked LCW implementation safe for
        // these trusted, locally generated smoke artifacts (never arbitrary input).
        std::vector<byte> dst(4*1024*1024);size_t actual=size_out;
        if(is_lzo){if(lzo(&src[offset],size_in,dst.data(),&actual,nullptr)!=0)throw "FA2 lzo2.dll rejected pack";}
        else actual=decode80c(&src[offset],dst.data(),size_in);
        if(actual!=size_out)throw "decoded length mismatch";
        output.insert(output.end(),dst.begin(),dst.begin()+actual);offset+=size_in;
        if(output.size()>4*1024*1024)throw "oversized map pack";
    }
    return output;
}
int main(int argc,char** argv) {
    try{
        if(argc!=2 && !(argc==3 && std::string(argv[2])=="--generic"))throw "usage: fa2-pack-probe <edited.map> [--generic]";
        const bool placements=argc==2;
        std::ifstream input(argv[1],std::ios::binary);if(!input)throw "map open failed";
        std::string text((std::istreambuf_iterator<char>(input)),{});
        HMODULE dll=LoadLibraryA("lzo2.dll");if(!dll)throw "FA2 lzo2.dll not found";
        auto lzo=reinterpret_cast<LzoDecode>(GetProcAddress(dll,"lzo1x_decompress_safe"));if(!lzo)throw "lzo export not found";
        auto terrain=unpack(packed(text,"IsoMapPack5"),lzo,true);
        auto overlay=unpack(packed(text,"OverlayPack"),lzo,false);
        auto frames=unpack(packed(text,"OverlayDataPack"),lzo,false);
        if(overlay.size()!=512*512||frames.size()!=512*512)throw "overlay dimensions";
        unsigned records=0;bool authored[3]={};
        for(size_t i=0;i+11<=terrain.size();i+=11){auto x=word(&terrain[i]);auto y=word(&terrain[i+2]);if(!x&&!y)break;records++;if(y==85&&x>=86&&x<=88)authored[x-86]=true;}
        if(placements){
        if(!authored[0]||!authored[1]||!authored[2])throw "painted terrain missing";
        if(overlay[85*512+91]!=102||frames[85*512+91]!=11)throw "ore not at expected cell";
        if(overlay[87*512+91]!=27)throw "gems not at expected cell";
        if(text.find("Neutral,GAPOWR,256,86,88,")==std::string::npos)throw "building missing";
        if(text.find("Neutral,E1,256,89,88,")==std::string::npos)throw "infantry missing";
        if(text.find("0=85086")==std::string::npos)throw "waypoint missing";
        }
        if(!records)throw "empty terrain pack";
        std::printf("{\"passed\":true,\"decoder\":\"FinalAlert2 XCC decode80c + bundled lzo2.dll\",\"terrain_records\":%u,\"terrain_bytes\":%zu,\"overlay_bytes\":%zu,\"frame_bytes\":%zu,\"placements\":%s,\"waypoint\":%s}\n",records,terrain.size(),overlay.size(),frames.size(),placements?"true":"false",placements?"true":"false");
        FreeLibrary(dll);return 0;
    }catch(const char* error){std::fprintf(stderr,"%s\n",error);return 1;}
}
